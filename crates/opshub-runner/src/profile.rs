use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

fn default_kind() -> String {
    "generic".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WinSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for WinSize {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 32,
        }
    }
}
