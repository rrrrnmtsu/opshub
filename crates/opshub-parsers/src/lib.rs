//! Engine-specific log tailers that extract token usage from each CLI's own
//! JSONL output and map it to a common [`CostSample`]. Pricing lives alongside
//! so both the runner and the TUI can share the same USD estimates.

pub mod claude;
pub mod codex;
pub mod pricing;

use serde::{Deserialize, Serialize};

/// Which upstream CLI produced a [`CostSample`]. Matches the `kind` field on
/// an `AgentProfile` so the runner can pick a tailer without a separate enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    ClaudeCode,
    Codex,
}

impl Engine {
    pub fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "claude_code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

/// One observed usage datum (a single model turn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSample {
    /// opshub's own `session.id`, assigned by the runner at spawn time.
    pub opshub_session_id: String,
    pub engine: Engine,
    /// Unix epoch ms, from the source log's own timestamp when parseable.
    pub ts_ms: i64,
    pub model: String,
    pub input_tok: i64,
    pub output_tok: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub usd_estimate: f64,
}

/// Pull-based tailer. Callers poll on a fixed cadence; returning `Ok(None)`
/// means "no new sample yet, try again later" (not an error).
pub trait Tailer: Send {
    fn next(&mut self) -> anyhow::Result<Option<CostSample>>;
}
