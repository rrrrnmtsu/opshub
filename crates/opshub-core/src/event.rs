use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Stdin,
    Stdout,
    Stderr,
    Hook,
    Mcp,
    Cost,
    ToolUse,
    Meta,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Stdin => "stdin",
            EventKind::Stdout => "stdout",
            EventKind::Stderr => "stderr",
            EventKind::Hook => "hook",
            EventKind::Mcp => "mcp",
            EventKind::Cost => "cost",
            EventKind::ToolUse => "tool_use",
            EventKind::Meta => "meta",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub session_id: String,
    pub ts_ms: i64,
    pub kind: EventKind,
    pub payload: Option<Vec<u8>>,
    pub text: Option<String>,
}

impl Event {
    pub fn new(session_id: impl Into<String>, kind: EventKind) -> Self {
        Self {
            session_id: session_id.into(),
            ts_ms: now_ms(),
            kind,
            payload: None,
            text: None,
        }
    }

    pub fn with_payload(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.payload = Some(bytes.into());
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
