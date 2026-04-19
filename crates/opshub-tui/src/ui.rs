use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::grid;

/// Upper bound on header rows (title + metrics). [`app::resize_agents_for`]
/// uses this to reserve body space once; the layout may still give the header
/// fewer rows when metrics are hidden.
pub const HEADER_MAX_ROWS: u16 = 2;

pub fn render(frame: &mut Frame, state: &AppState) {
    let root = frame.area();
    let header_rows: u16 = if state.show_metrics { 2 } else { 1 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(root);

    render_header(frame, chunks[0], state);
    render_grid(frame, chunks[1], state);
    render_footer(frame, chunks[2], state);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut title_spans: Vec<Span> = vec![
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
        title_spans.push(Span::styled(label, style));
        title_spans.push(Span::raw(" "));
    }

    if !state.show_metrics || area.height < 2 {
        frame.render_widget(Paragraph::new(Line::from(title_spans)), area);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);
    frame.render_widget(Paragraph::new(Line::from(title_spans)), rows[0]);
    frame.render_widget(Paragraph::new(Line::from(metrics_spans(state))), rows[1]);
}

fn metrics_spans(state: &AppState) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(state.agents.len() * 4);
    let dim = Style::default().fg(Color::DarkGray);
    let usd = Style::default().fg(Color::Green);
    let tok = Style::default().fg(Color::Yellow);
    for (i, agent) in state.agents.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", dim));
        }
        let has_data = agent.cost.usd_total > 0.0
            || !agent.cost.window.is_empty()
            || agent.cost.model.is_some();
        spans.push(Span::styled(format!("{}: ", agent.label), dim));
        if !has_data {
            spans.push(Span::styled("— no cost data yet —".to_string(), dim));
            continue;
        }
        spans.push(Span::styled(format!("${:.4}", agent.cost.usd_total), usd));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{:.0} tok/s", agent.cost.tok_per_sec),
            tok,
        ));
        if let Some(model) = &agent.cost.model {
            spans.push(Span::styled(format!("  · {model}"), dim));
        }
    }
    spans
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
        Span::styled("Ctrl-M", Style::default().fg(Color::Yellow)),
        Span::raw(" metrics  "),
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
