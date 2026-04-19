//! Thin wrapper around [`vt100::Parser`] that (a) turns the parser's 2D
//! grid into styled ratatui rows, and (b) auto-replies to a handful of
//! terminal queries (DSR, DA) so interactive CLIs like `codex` and `claude`
//! don't hang on startup waiting for a terminal that never answers.
//!
//! We intentionally only implement the minimum query set we've seen real
//! agents use. Anything more exotic (cursor color, background color, mouse
//! encoding) can be added as users report it.
//!
//! Output is reported as an aggregate `Vec<u8>` from [`Screen::process`].
//! The caller is expected to forward that back to the PTY via the runner's
//! `write_input`.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// vt100-backed screen buffer for one agent pane.
pub struct Screen {
    parser: vt100::Parser,
}

impl Screen {
    /// `scrollback` is measured in rows. 2000 matches the old `LineBuffer`
    /// cap and is plenty for eyeballing an agent's recent output without
    /// turning the per-pane allocation into a profile of the day's work.
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback),
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Feed raw PTY bytes. Returns any reply bytes that the child expects
    /// from its terminal (cursor-position report, device attributes, etc.);
    /// callers must write these back to the PTY.
    #[must_use]
    pub fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        // Feed the parser first so cursor position queries reflect the state
        // after any positioning escape sequences in the same byte chunk. In
        // practice the CLIs we target send their DSR-6 at a single clean
        // startup moment, so treating the end-of-chunk cursor as "the
        // cursor at query time" is accurate enough.
        self.parser.process(bytes);
        let mut replies: Vec<u8> = Vec::new();
        for query in extract_queries(bytes) {
            match query {
                Query::CursorPosition => {
                    // vt100 reports (row, col) 0-indexed; DSR uses 1-indexed.
                    let (row, col) = self.parser.screen().cursor_position();
                    let r = row.saturating_add(1);
                    let c = col.saturating_add(1);
                    replies.extend_from_slice(format!("\x1b[{r};{c}R").as_bytes());
                }
                Query::Status => replies.extend_from_slice(b"\x1b[0n"),
                Query::PrimaryDa => replies.extend_from_slice(b"\x1b[?6c"),
                Query::SecondaryDa => replies.extend_from_slice(b"\x1b[>0;0;0c"),
            }
        }
        replies
    }

    pub fn render_lines(&self, area_rows: usize, area_cols: usize) -> Vec<Line<'static>> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let rows_to_draw = area_rows.min(rows as usize);
        let cols_to_draw = area_cols.min(cols as usize);

        let mut lines: Vec<Line<'static>> = Vec::with_capacity(rows_to_draw);
        for row in 0..rows_to_draw as u16 {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut current_style = Style::default();
            let mut buf = String::new();
            for col in 0..cols_to_draw as u16 {
                let cell = match screen.cell(row, col) {
                    Some(c) => c,
                    None => break,
                };
                if cell.is_wide_continuation() {
                    // Skip — the wide character's first half already emitted
                    // the full grapheme.
                    continue;
                }
                let style = cell_style(cell);
                let text = if cell.has_contents() {
                    cell.contents().to_string()
                } else {
                    " ".to_string()
                };
                if style != current_style && !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), current_style));
                    current_style = style;
                } else if buf.is_empty() {
                    current_style = style;
                }
                buf.push_str(&text);
            }
            if !buf.is_empty() {
                spans.push(Span::styled(buf, current_style));
            }
            lines.push(Line::from(spans));
        }
        lines
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut s = Style::default();
    s = s.fg(map_color(cell.fgcolor()));
    let bg = map_color(cell.bgcolor());
    if !matches!(bg, Color::Reset) {
        s = s.bg(bg);
    }
    let mut mods = Modifier::empty();
    if cell.bold() {
        mods |= Modifier::BOLD;
    }
    if cell.italic() {
        mods |= Modifier::ITALIC;
    }
    if cell.underline() {
        mods |= Modifier::UNDERLINED;
    }
    if cell.dim() {
        mods |= Modifier::DIM;
    }
    if cell.inverse() {
        mods |= Modifier::REVERSED;
    }
    if !mods.is_empty() {
        s = s.add_modifier(mods);
    }
    s
}

fn map_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Kinds of terminal query we know how to answer. Anything not enumerated
/// here falls through to `vt100::Parser` unmodified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    CursorPosition,
    Status,
    PrimaryDa,
    SecondaryDa,
}

/// Scan raw PTY output for control sequences that expect a reply. We look
/// for exact byte prefixes rather than running a full ANSI parser — the
/// query set is tiny and this keeps the hot path allocation-free.
fn extract_queries(bytes: &[u8]) -> Vec<Query> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] != 0x1b || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        // Read the parameter span up to an ASCII final byte (0x40..=0x7e).
        let mut j = i + 2;
        let mut private = false;
        if j < bytes.len() && bytes[j] == b'>' {
            private = true;
            j += 1;
        }
        while j < bytes.len() && bytes[j].is_ascii_digit() || (j < bytes.len() && bytes[j] == b';')
        {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let params = &bytes[i + 2 + if private { 1 } else { 0 }..j];
        let final_byte = bytes[j];
        match (final_byte, params, private) {
            (b'n', b"6", false) => out.push(Query::CursorPosition),
            (b'n', b"5", false) => out.push(Query::Status),
            (b'c', b"", false) => out.push(Query::PrimaryDa),
            (b'c', b"0", false) => out.push(Query::PrimaryDa),
            (b'c', b"", true) => out.push(Query::SecondaryDa),
            (b'c', b"0", true) => out.push(Query::SecondaryDa),
            _ => {}
        }
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_renders() {
        let mut s = Screen::new(4, 20, 100);
        let replies = s.process(b"hello world");
        assert!(replies.is_empty());
        let lines = s.render_lines(4, 20);
        assert_eq!(lines.len(), 4);
        // First row should contain "hello world" flattened.
        let flat: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(flat.starts_with("hello world"), "got {flat:?}");
    }

    #[test]
    fn dsr_cursor_position_reply() {
        let mut s = Screen::new(4, 20, 100);
        // Move cursor to row 2 col 5, then request position.
        let replies = s.process(b"\x1b[2;5H\x1b[6n");
        assert_eq!(replies, b"\x1b[2;5R");
    }

    #[test]
    fn status_and_da_queries() {
        let mut s = Screen::new(4, 20, 100);
        let replies = s.process(b"\x1b[5n\x1b[c");
        // Status OK then primary DA (VT102).
        let expected: Vec<u8> = b"\x1b[0n\x1b[?6c".to_vec();
        assert_eq!(replies, expected);
    }

    #[test]
    fn alt_screen_content_renders() {
        let mut s = Screen::new(4, 20, 100);
        let _ = s.process(b"\x1b[?1049h"); // enter alt screen
        let _ = s.process(b"alt-screen hi");
        let lines = s.render_lines(4, 20);
        let flat: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(flat.starts_with("alt-screen hi"), "got {flat:?}");
    }
}
