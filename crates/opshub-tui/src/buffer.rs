use opshub_core::ansi;
use std::collections::VecDeque;

/// Per-agent scrollback buffer.
///
/// We don't emulate a full VT100 — just keep the last `cap` lines of
/// ANSI-stripped text so the user can see what their agent is doing. A proper
/// semi-emulator (colors, cursor positioning) arrives in v0.0.3 once the
/// grid + input routing are battle-tested.
pub struct LineBuffer {
    lines: VecDeque<String>,
    cap: usize,
    /// Incomplete tail — bytes that arrived without a terminating newline.
    pending: String,
}

impl LineBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(cap.min(1024)),
            cap,
            pending: String::new(),
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let text = ansi::strip(bytes);
        for ch in text.chars() {
            match ch {
                '\n' => self.commit_pending(),
                '\r' => {
                    // Emulate carriage-return-as-line-clear. Many CLIs use
                    // `\r` to redraw a progress line in place; without this
                    // the scrollback fills with noise.
                    self.pending.clear();
                }
                c if (c as u32) < 0x20 && c != '\t' => {
                    // Drop other C0 control chars; ANSI stripper already
                    // handled most of them but some may remain.
                }
                c => self.pending.push(c),
            }
        }
    }

    fn commit_pending(&mut self) {
        let line = std::mem::take(&mut self.pending);
        self.lines.push_back(line);
        while self.lines.len() > self.cap {
            self.lines.pop_front();
        }
    }

    /// Borrow the last `n` lines (newest last). Includes the in-progress line
    /// so partial output is visible.
    pub fn tail(&self, n: usize) -> Vec<&str> {
        let total = self.lines.len() + if self.pending.is_empty() { 0 } else { 1 };
        let start = total.saturating_sub(n);
        let mut out: Vec<&str> = self
            .lines
            .iter()
            .skip(start.min(self.lines.len()))
            .map(|s| s.as_str())
            .collect();
        if !self.pending.is_empty() && out.len() < n {
            out.push(&self.pending);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_newline() {
        let mut b = LineBuffer::new(10);
        b.push_bytes(b"hello\nworld\n");
        let tail = b.tail(10);
        assert_eq!(tail, vec!["hello", "world"]);
    }

    #[test]
    fn keeps_partial_tail_visible() {
        let mut b = LineBuffer::new(10);
        b.push_bytes(b"line1\npartial");
        assert_eq!(b.tail(10), vec!["line1", "partial"]);
    }

    #[test]
    fn carriage_return_rewrites_current_line() {
        let mut b = LineBuffer::new(10);
        b.push_bytes(b"progress 10%\rprogress 90%\n");
        assert_eq!(b.tail(10), vec!["progress 90%"]);
    }

    #[test]
    fn strips_ansi() {
        let mut b = LineBuffer::new(10);
        b.push_bytes(b"\x1b[31mred\x1b[0m\n");
        assert_eq!(b.tail(10), vec!["red"]);
    }

    #[test]
    fn honors_cap() {
        let mut b = LineBuffer::new(2);
        b.push_bytes(b"a\nb\nc\nd\n");
        assert_eq!(b.tail(10), vec!["c", "d"]);
    }
}
