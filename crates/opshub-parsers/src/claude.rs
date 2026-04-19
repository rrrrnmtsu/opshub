//! Tailer for Claude Code's transcript JSONL files.
//!
//! Claude Code writes one JSONL per session at
//! `~/.claude/projects/<cwd-encoded>/<sessionId>.jsonl` where the cwd encoding
//! replaces `/` with `-`. Each `type: "assistant"` line carries a
//! `message.usage` block with input/output/cache token counts. We convert
//! those into [`CostSample`]s using [`pricing::estimate`].

use std::fs::{File, Metadata};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{pricing, CostSample, Engine, Tailer};

/// Turn `/Users/remma/dev/opshub` into `-Users-remma-dev-opshub`.
///
/// Claude Code's on-disk layout mirrors this — we only need to match it,
/// not invent it. On platforms without a leading `/` (theoretically
/// Windows) we fall back to a replace that still produces a valid name.
pub fn encode_cwd(cwd: &Path) -> String {
    let s = cwd.to_string_lossy();
    s.replace(std::path::MAIN_SEPARATOR, "-")
}

pub struct ClaudeTranscriptTailer {
    opshub_session_id: String,
    dir: PathBuf,
    launch_ts_ms: i64,
    selected: Option<PathBuf>,
    reader: Option<BufReader<File>>,
    /// Track the active file's modified-at so we notice truncation/rotation.
    selected_mtime: Option<std::time::SystemTime>,
}

impl ClaudeTranscriptTailer {
    pub fn new(opshub_session_id: String, cwd: &Path, launch_ts_ms: i64) -> Self {
        let dir = claude_projects_dir().join(encode_cwd(cwd));
        Self {
            opshub_session_id,
            dir,
            launch_ts_ms,
            selected: None,
            reader: None,
            selected_mtime: None,
        }
    }

    /// For tests: override the projects root dir.
    pub fn with_dir(opshub_session_id: String, dir: PathBuf, launch_ts_ms: i64) -> Self {
        Self {
            opshub_session_id,
            dir,
            launch_ts_ms,
            selected: None,
            reader: None,
            selected_mtime: None,
        }
    }

    fn ensure_reader(&mut self) -> Result<bool> {
        if self.reader.is_some() {
            return Ok(true);
        }
        if !self.dir.exists() {
            return Ok(false);
        }
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        for entry in std::fs::read_dir(&self.dir).context("read_dir claude projects")? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let meta: Metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = match meta.modified() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime_ms = to_ms(mtime);
            if mtime_ms < self.launch_ts_ms {
                continue;
            }
            if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best = Some((path, mtime));
            }
        }
        let Some((path, mtime)) = best else {
            return Ok(false);
        };
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut reader = BufReader::new(file);
        // Start at the head so we don't miss lines that were written between
        // launch and the first poll.
        reader.seek(SeekFrom::Start(0))?;
        debug!(path = %path.display(), "claude tailer attached");
        self.selected = Some(path);
        self.selected_mtime = Some(mtime);
        self.reader = Some(reader);
        Ok(true)
    }
}

impl Tailer for ClaudeTranscriptTailer {
    fn next(&mut self) -> Result<Option<CostSample>> {
        if !self.ensure_reader()? {
            return Ok(None);
        }
        let reader = self.reader.as_mut().expect("ensured reader");
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .context("read transcript line")?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            match parse_line(trimmed, &self.opshub_session_id) {
                Ok(Some(sample)) => return Ok(Some(sample)),
                Ok(None) => continue,
                Err(e) => {
                    warn!(error = %e, "claude transcript parse failure, skipping line");
                    continue;
                }
            }
        }
    }
}

fn parse_line(line: &str, opshub_session_id: &str) -> Result<Option<CostSample>> {
    let v: serde_json::Value = serde_json::from_str(line).context("parse json")?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return Ok(None);
    }
    let msg = match v.get("message") {
        Some(m) => m,
        None => return Ok(None),
    };
    let usage = match msg.get("usage") {
        Some(u) => u,
        None => return Ok(None),
    };
    let model = msg
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();
    let input = usage
        .get("input_tokens")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let cache_write = usage
        .get("cache_creation_input_tokens")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    // Skip "empty" usage rows (Claude emits zero-token assistant frames for
    // permission prompts, thinking deltas etc.). No signal, just noise in the
    // tok/s window.
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return Ok(None);
    }
    let ts_ms = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_iso8601_ms)
        .unwrap_or_else(now_ms);
    let usd = pricing::estimate(&model, input, output, cache_read, cache_write);
    Ok(Some(CostSample {
        opshub_session_id: opshub_session_id.to_string(),
        engine: Engine::ClaudeCode,
        ts_ms,
        model,
        input_tok: input,
        output_tok: output,
        cache_read,
        cache_write,
        usd_estimate: usd,
    }))
}

fn claude_projects_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".claude").join("projects")
    } else {
        PathBuf::from(".claude").join("projects")
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

/// Parse a subset of ISO-8601 (`YYYY-MM-DDTHH:MM:SS[.fff]Z`) into epoch ms
/// without pulling in a chrono/time dependency. All samples we've seen are
/// UTC with a trailing `Z`, so we lean on that; anything else falls through.
pub(crate) fn parse_iso8601_ms_public(s: &str) -> Option<i64> {
    parse_iso8601_ms(s)
}

fn parse_iso8601_ms(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    // Accept "YYYY-MM-DDTHH:MM:SS" plus an optional ".fffff" fractional part.
    let (date, time) = s.split_once('T')?;
    let mut dparts = date.split('-');
    let year: i64 = dparts.next()?.parse().ok()?;
    let month: u32 = dparts.next()?.parse().ok()?;
    let day: u32 = dparts.next()?.parse().ok()?;

    let (hms, frac) = match time.split_once('.') {
        Some((h, f)) => (h, f),
        None => (time, "0"),
    };
    let mut tparts = hms.split(':');
    let hour: u32 = tparts.next()?.parse().ok()?;
    let min: u32 = tparts.next()?.parse().ok()?;
    let sec: u32 = tparts.next()?.parse().ok()?;

    // Civil-day-to-epoch using Howard Hinnant's algorithm (public domain).
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (m as i64) + 2) / 5 + (day as i64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    let frac_ms: i64 = {
        // Pad/truncate fractional digits to 3 (milliseconds).
        let f = frac.chars().take(3).collect::<String>();
        let padded = format!("{f:0<3}");
        padded.parse().unwrap_or(0)
    };
    Some(secs * 1000 + frac_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn encodes_cwd_matching_claude_layout() {
        let p = Path::new("/Users/remma/dev/opshub");
        assert_eq!(encode_cwd(p), "-Users-remma-dev-opshub");
    }

    #[test]
    fn iso8601_roundtrip() {
        let got = parse_iso8601_ms("2026-04-19T10:00:00.500Z").unwrap();
        // Sanity: sometime in April 2026 should be ~1776... * 1000 + 500.
        assert!(got > 1_700_000_000_000);
        assert_eq!(got % 1000, 500);
    }

    #[test]
    fn parses_real_claude_usage_line() {
        let line = r#"{"type":"assistant","timestamp":"2026-04-19T10:00:00.000Z","sessionId":"abc","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":3,"cache_creation_input_tokens":38434,"cache_read_input_tokens":0,"output_tokens":42}}}"#;
        let sample = parse_line(line, "opshub-sess-1").unwrap().unwrap();
        assert_eq!(sample.engine, Engine::ClaudeCode);
        assert_eq!(sample.opshub_session_id, "opshub-sess-1");
        assert_eq!(sample.model, "claude-sonnet-4-6");
        assert_eq!(sample.input_tok, 3);
        assert_eq!(sample.output_tok, 42);
        assert_eq!(sample.cache_write, 38434);
        assert_eq!(sample.cache_read, 0);
        assert!(sample.usd_estimate > 0.0);
    }

    #[test]
    fn non_assistant_lines_return_none() {
        let line = r#"{"type":"permission-mode","permissionMode":"plan","sessionId":"abc"}"#;
        assert!(parse_line(line, "s").unwrap().is_none());
    }

    #[test]
    fn zero_usage_lines_return_none() {
        let line = r#"{"type":"assistant","message":{"model":"opus","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        assert!(parse_line(line, "s").unwrap().is_none());
    }

    #[test]
    fn tailer_picks_newest_jsonl_and_emits_samples() {
        let tmp = tempdir();
        let dir = tmp.join("-Users-remma-dev-opshub");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session-fresh.jsonl");
        {
            let mut f = File::create(&file).unwrap();
            writeln!(f, r#"{{"type":"permission-mode","permissionMode":"plan"}}"#).unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","message":{{"model":"claude-sonnet-4-6","usage":{{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            )
            .unwrap();
        }
        let launch = to_ms(std::time::SystemTime::now()) - 60_000;
        let mut tailer = ClaudeTranscriptTailer::with_dir("sess".into(), tmp.clone(), launch);
        // The tailer's with_dir takes the root; we need it to look inside the
        // encoded subdir. For the test we stuff everything at the root and
        // encode the cwd to the actual subdir name.
        tailer.dir = dir;
        let first = tailer.next().unwrap();
        assert!(first.is_some(), "expected at least one sample");
        let s = first.unwrap();
        assert_eq!(s.input_tok, 10);
        assert_eq!(s.output_tok, 20);
        // No more lines → None.
        assert!(tailer.next().unwrap().is_none());
    }

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "opshub-parsers-test-{}",
            std::process::id() as u64 * 1000 + fastrand()
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
