//! Tailer for the Codex CLI's rollout JSONL files.
//!
//! Codex writes to `~/.codex/sessions/YYYY/MM/DD/rollout-<iso>-<uuid>.jsonl`.
//! Each line is a typed record: `session_meta`, `turn_context` (carrying the
//! `model` for subsequent turns) and `event_msg` with a `token_count` payload
//! that holds `last_token_usage`. We treat `last_token_usage` as a per-turn
//! delta (the field is designed for exactly that — using `total_token_usage`
//! would double-count tokens across the session).

use std::fs::{File, Metadata};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{pricing, CostSample, Engine, Tailer};

pub struct CodexSessionTailer {
    opshub_session_id: String,
    root: PathBuf,
    launch_ts_ms: i64,
    reader: Option<BufReader<File>>,
    selected: Option<PathBuf>,
    last_model: Option<String>,
}

impl CodexSessionTailer {
    pub fn new(opshub_session_id: String, launch_ts_ms: i64) -> Self {
        Self {
            opshub_session_id,
            root: codex_sessions_dir(),
            launch_ts_ms,
            reader: None,
            selected: None,
            last_model: None,
        }
    }

    /// For tests: override the root dir so we don't touch the real `~/.codex`.
    pub fn with_root(opshub_session_id: String, root: PathBuf, launch_ts_ms: i64) -> Self {
        Self {
            opshub_session_id,
            root,
            launch_ts_ms,
            reader: None,
            selected: None,
            last_model: None,
        }
    }

    fn ensure_reader(&mut self) -> Result<bool> {
        if self.reader.is_some() {
            return Ok(true);
        }
        if !self.root.exists() {
            return Ok(false);
        }
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        walk_jsonl(&self.root, &mut |path: &Path, meta: &Metadata| {
            let mtime = match meta.modified() {
                Ok(m) => m,
                Err(_) => return,
            };
            if to_ms(mtime) < self.launch_ts_ms {
                return;
            }
            if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best = Some((path.to_path_buf(), mtime));
            }
        })?;
        let Some((path, _)) = best else {
            return Ok(false);
        };
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(0))?;
        debug!(path = %path.display(), "codex tailer attached");
        self.selected = Some(path);
        self.reader = Some(reader);
        Ok(true)
    }
}

impl Tailer for CodexSessionTailer {
    fn next(&mut self) -> Result<Option<CostSample>> {
        if !self.ensure_reader()? {
            return Ok(None);
        }
        let reader = self.reader.as_mut().expect("ensured reader");
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).context("read rollout line")?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            match process_line(trimmed, &mut self.last_model, &self.opshub_session_id) {
                Ok(Some(sample)) => return Ok(Some(sample)),
                Ok(None) => continue,
                Err(e) => {
                    warn!(error = %e, "codex rollout parse failure, skipping line");
                    continue;
                }
            }
        }
    }
}

/// Public for tests. Mutates `last_model` when a `turn_context` is seen so
/// the caller can accumulate state across calls.
pub(crate) fn process_line(
    line: &str,
    last_model: &mut Option<String>,
    opshub_session_id: &str,
) -> Result<Option<CostSample>> {
    let v: serde_json::Value = serde_json::from_str(line).context("parse json")?;
    let rec_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match rec_type {
        "turn_context" => {
            if let Some(model) = v
                .get("payload")
                .and_then(|p| p.get("model"))
                .and_then(|m| m.as_str())
            {
                *last_model = Some(model.to_string());
            }
            Ok(None)
        }
        "event_msg" => {
            let payload = match v.get("payload") {
                Some(p) => p,
                None => return Ok(None),
            };
            if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                return Ok(None);
            }
            let info = match payload.get("info") {
                Some(i) => i,
                None => return Ok(None),
            };
            let last = match info.get("last_token_usage") {
                Some(u) => u,
                None => return Ok(None),
            };
            let input = last
                .get("input_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            let cached = last
                .get("cached_input_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            let output = last
                .get("output_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            let reasoning = last
                .get("reasoning_output_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            let output_total = output + reasoning;
            if input == 0 && cached == 0 && output_total == 0 {
                return Ok(None);
            }
            let model = last_model.clone().unwrap_or_else(|| "unknown".to_string());
            let ts_ms = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(crate::claude::parse_iso8601_ms_public)
                .unwrap_or_else(now_ms);
            // Codex's `input_tokens` already *includes* cached input, so we
            // charge the uncached portion at the normal rate and the cached
            // portion at the cache-read rate (closest match to OpenAI's
            // pricing split).
            let fresh_input = (input - cached).max(0);
            let usd = pricing::estimate(&model, fresh_input, output_total, cached, 0);
            Ok(Some(CostSample {
                opshub_session_id: opshub_session_id.to_string(),
                engine: Engine::Codex,
                ts_ms,
                model,
                input_tok: fresh_input,
                output_tok: output_total,
                cache_read: cached,
                cache_write: 0,
                usd_estimate: usd,
            }))
        }
        _ => Ok(None),
    }
}

fn codex_sessions_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".codex").join("sessions")
    } else {
        PathBuf::from(".codex").join("sessions")
    }
}

fn to_ms(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    to_ms(std::time::SystemTime::now())
}

/// Bounded-depth walk (YYYY/MM/DD plus files) so we don't blow the stack on
/// an unexpected directory layout. Anything deeper is ignored.
fn walk_jsonl(root: &Path, visit: &mut dyn FnMut(&Path, &Metadata)) -> Result<()> {
    walk_inner(root, 0, visit)
}

fn walk_inner(dir: &Path, depth: usize, visit: &mut dyn FnMut(&Path, &Metadata)) -> Result<()> {
    if depth > 4 {
        return Ok(());
    }
    let iter = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return Ok(()),
    };
    for entry in iter.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            let _ = walk_inner(&path, depth + 1, visit);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            visit(&path, &meta);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn turn_context_updates_model() {
        let mut model = None;
        let line = r#"{"type":"turn_context","payload":{"cwd":"/tmp","model":"gpt-5.4"}}"#;
        let res = process_line(line, &mut model, "s").unwrap();
        assert!(res.is_none());
        assert_eq!(model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn token_count_emits_sample_with_last_model() {
        let mut model = Some("gpt-5.4".to_string());
        let line = r#"{"timestamp":"2026-03-16T01:52:54.260Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":21938,"cached_input_tokens":3456,"output_tokens":373,"reasoning_output_tokens":106,"total_tokens":22311},"total_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":0},"model_context_window":258400}}}"#;
        let sample = process_line(line, &mut model, "opshub-1").unwrap().unwrap();
        assert_eq!(sample.engine, Engine::Codex);
        assert_eq!(sample.model, "gpt-5.4");
        // fresh_input = 21938 - 3456
        assert_eq!(sample.input_tok, 18482);
        // output + reasoning
        assert_eq!(sample.output_tok, 479);
        assert_eq!(sample.cache_read, 3456);
        assert!(sample.usd_estimate > 0.0);
    }

    #[test]
    fn zero_usage_emits_nothing() {
        let mut model = Some("gpt-5.4".to_string());
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":0}}}}"#;
        assert!(process_line(line, &mut model, "s").unwrap().is_none());
    }

    #[test]
    fn tailer_reads_file_from_nested_dir() {
        let tmp = tempdir();
        let dir = tmp.join("2026").join("03").join("16");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rollout-2026-03-16T10-52-39-abcd.jsonl");
        let mut f = File::create(&file).unwrap();
        writeln!(f, r#"{{"type":"session_meta","payload":{{}}}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"turn_context","payload":{{"model":"gpt-5.4"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150}}}}}}}}"#
        )
        .unwrap();
        drop(f);

        let launch = to_ms(std::time::SystemTime::now()) - 60_000;
        let mut tailer = CodexSessionTailer::with_root("sess".into(), tmp, launch);
        let s = tailer.next().unwrap().expect("expected a sample");
        assert_eq!(s.engine, Engine::Codex);
        assert_eq!(s.input_tok, 100);
        assert_eq!(s.output_tok, 50);
        assert!(tailer.next().unwrap().is_none());
    }

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "opshub-codex-test-{}-{}",
            std::process::id(),
            fastrand()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn fastrand() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0)
    }
}
