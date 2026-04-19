use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::grid;

pub fn render(frame: &mut Frame, state: &AppState) {
    let root = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(root);

    render_header(frame, chunks[0], state);
    render_grid(frame, chunks[1], state);
    render_footer(frame, chunks[2], state);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut spans: Vec<Span> = vec![
        Span::styled(
            "opshub ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "({} agent{}) ",
                state.agents.len(),
                plural(state.agents.len())
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    for (i, a) in state.agents.iter().enumerate() {
        let label = format!(" {} ", a.label);
        let style = if i == state.selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if a.exit_code.is_some() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_grid(frame: &mut Frame, area: Rect, state: &AppState) {
    let cells = grid::tile(area, state.agents.len());
    for (i, (agent, cell)) in state.agents.iter().zip(cells.iter()).enumerate() {
        let focused = i == state.selected;
        let title = match agent.exit_code {
            None => format!(" {} ", agent.label),
            Some(code) => format!(" {} [exited {code}] ", agent.label),
        };
        let border_style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        let inner = block.inner(*cell);
        frame.render_widget(block, *cell);

        let rows_available = inner.height as usize;
        let lines: Vec<Line> = agent
            .buffer
            .tail(rows_available)
            .into_iter()
            .map(|s| Line::from(Span::raw(s.to_string())))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let hint = Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" next  "),
        Span::styled("Ctrl-Q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::styled("·", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(&state.status, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(hint), area);
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
