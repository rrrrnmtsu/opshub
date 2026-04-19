-- opshub schema v0.0.1
-- Primary store for agent sessions, event timeline, cost attribution, and
-- inter-agent message traffic. All timestamps are unix epoch milliseconds.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS agent (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    profile     TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS session (
    id              TEXT PRIMARY KEY,
    agent_id        TEXT NOT NULL REFERENCES agent(id) ON DELETE CASCADE,
    cwd             TEXT,
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    exit_code       INTEGER,
    tmux_pane       TEXT,
    parent_session  TEXT REFERENCES session(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_session_agent ON session(agent_id);
CREATE INDEX IF NOT EXISTS idx_session_parent ON session(parent_session);

CREATE TABLE IF NOT EXISTS event (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    ts          INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('stdin','stdout','stderr','hook','mcp','cost','tool_use','meta')),
    payload     BLOB,
    text        TEXT
);
CREATE INDEX IF NOT EXISTS idx_event_session_ts ON event(session_id, ts);
CREATE INDEX IF NOT EXISTS idx_event_kind ON event(kind);

-- Full-text index over ANSI-stripped text content.
CREATE VIRTUAL TABLE IF NOT EXISTS event_fts USING fts5(
    text,
    content='event',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS event_fts_ai AFTER INSERT ON event BEGIN
    INSERT INTO event_fts(rowid, text) VALUES (new.id, coalesce(new.text, ''));
END;
CREATE TRIGGER IF NOT EXISTS event_fts_ad AFTER DELETE ON event BEGIN
    INSERT INTO event_fts(event_fts, rowid, text) VALUES('delete', old.id, coalesce(old.text, ''));
END;
CREATE TRIGGER IF NOT EXISTS event_fts_au AFTER UPDATE ON event BEGIN
    INSERT INTO event_fts(event_fts, rowid, text) VALUES('delete', old.id, coalesce(old.text, ''));
    INSERT INTO event_fts(rowid, text) VALUES (new.id, coalesce(new.text, ''));
END;

CREATE TABLE IF NOT EXISTS cost_event (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    ts            INTEGER NOT NULL,
    model         TEXT,
    input_tok     INTEGER,
    output_tok    INTEGER,
    cache_read    INTEGER,
    cache_write   INTEGER,
    usd_estimate  REAL
);
CREATE INDEX IF NOT EXISTS idx_cost_session_ts ON cost_event(session_id, ts);

CREATE TABLE IF NOT EXISTS agent_message (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent  TEXT,
    to_agent    TEXT,
    ts          INTEGER NOT NULL,
    via         TEXT,
    summary     TEXT,
    payload     TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_message_ts ON agent_message(ts);

CREATE TABLE IF NOT EXISTS artifact (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    action      TEXT NOT NULL CHECK (action IN ('write','edit','delete','read')),
    ts          INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifact_session ON artifact(session_id);

-- Track schema version so future migrations can skip applied steps.
CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  INTEGER NOT NULL
);
INSERT OR IGNORE INTO schema_version(version, applied_at) VALUES (1, strftime('%s','now') * 1000);
