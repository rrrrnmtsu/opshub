use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::event::{now_ms, Event, EventKind};

const SCHEMA_SQL: &str = include_str!("../migrations/0001_init.sql");

/// Thin synchronous wrapper over a single SQLite connection.
///
/// Wrapped in Arc<Mutex<_>> so async callers can `spawn_blocking` without
/// threading a `&mut` through every layer. SQLite with WAL mode handles the
/// contention well for our write volume.
#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .with_context(|| format!("open sqlite at {}", path.as_ref().display()))?;
        conn.execute_batch(SCHEMA_SQL).context("apply schema")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("open in-memory sqlite")?;
        conn.execute_batch(SCHEMA_SQL).context("apply schema")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let v: Option<i64> = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(v.unwrap_or(0))
    }

    pub fn insert_agent(&self, id: &str, kind: &str, profile_json: &str) -> Result<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO agent(id, kind, profile, created_at) VALUES (?, ?, ?, ?)",
            params![id, kind, profile_json, now_ms()],
        )?;
        Ok(())
    }

    pub fn start_session(
        &self,
        id: &str,
        agent_id: &str,
        cwd: Option<&str>,
        tmux_pane: Option<&str>,
        parent_session: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "INSERT INTO session(id, agent_id, cwd, started_at, tmux_pane, parent_session) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![id, agent_id, cwd, now_ms(), tmux_pane, parent_session],
        )?;
        Ok(())
    }

    pub fn end_session(&self, id: &str, exit_code: Option<i32>) -> Result<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "UPDATE session SET ended_at = ?, exit_code = ? WHERE id = ?",
            params![now_ms(), exit_code, id],
        )?;
        Ok(())
    }

    pub fn insert_event(&self, ev: &Event) -> Result<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "INSERT INTO event(session_id, ts, kind, payload, text) VALUES (?, ?, ?, ?, ?)",
            params![
                ev.session_id,
                ev.ts_ms,
                ev.kind.as_str(),
                ev.payload.as_deref(),
                ev.text.as_deref(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// FTS5-backed search; returns (event_id, session_id, ts, text snippet).
    ///
    /// The query is treated as a phrase by default (wrapped in `"..."`) so
    /// punctuation like `-` or `.` doesn't collide with FTS5 operator syntax.
    /// If the caller already wraps tokens in double quotes we leave the
    /// query untouched — that escape hatch lets power users use MATCH
    /// operators (`AND`, `OR`, `NEAR`, prefix `*`, etc.) directly.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<(i64, String, i64, String)>> {
        let effective = if query.contains('"') {
            query.to_string()
        } else {
            format!("\"{query}\"")
        };
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT e.id, e.session_id, e.ts, e.text \
             FROM event_fts f JOIN event e ON e.id = f.rowid \
             WHERE event_fts MATCH ? \
             ORDER BY e.ts DESC \
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![effective, limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn count_events(&self, session_id: &str, kind: EventKind) -> Result<i64> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM event WHERE session_id = ? AND kind = ?",
            params![session_id, kind.as_str()],
            |row| row.get(0),
        )?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind};

    #[test]
    fn applies_schema_and_reports_version() {
        let s = Storage::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), 1);
    }

    #[test]
    fn insert_and_search_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_agent("agent-a", "claude_code", "{}").unwrap();
        s.start_session("sess-1", "agent-a", Some("/tmp"), None, None)
            .unwrap();

        let ev = Event::new("sess-1", EventKind::Stdout)
            .with_payload(b"hello".to_vec())
            .with_text("hello world");
        s.insert_event(&ev).unwrap();

        let hits = s.search("hello", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "sess-1");
        assert!(hits[0].3.contains("hello"));

        assert_eq!(s.count_events("sess-1", EventKind::Stdout).unwrap(), 1);
        assert_eq!(s.count_events("sess-1", EventKind::Stderr).unwrap(), 0);
    }

    #[test]
    fn foreign_key_cascades() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_agent("a", "generic", "{}").unwrap();
        s.start_session("s", "a", None, None, None).unwrap();
        let ev = Event::new("s", EventKind::Stdout).with_text("x");
        s.insert_event(&ev).unwrap();
        s.end_session("s", Some(0)).unwrap();
        // cascade on agent delete: event rows should disappear.
        let conn = s.conn.lock().unwrap();
        conn.execute("DELETE FROM agent WHERE id = 'a'", [])
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
