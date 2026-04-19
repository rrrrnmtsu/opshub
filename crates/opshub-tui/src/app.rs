use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use opshub_runner::{RunnerEvent, RunningAgent, WinSize};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::Stdout;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::buffer::LineBuffer;
use crate::grid;
use crate::ui;

const SCROLLBACK_LINES: usize = 2000;

pub struct AppOptions {
    pub agents: Vec<RunningAgent>,
    /// Informational — shown in the footer.
    pub db_label: Option<String>,
}

pub struct AgentView {
    pub label: String,
    pub buffer: LineBuffer,
    pub exit_code: Option<u32>,
}

pub struct AppState {
    pub agents: Vec<AgentView>,
    pub selected: usize,
    pub status: String,
}

enum AppMsg {
    Bytes { agent: usize, bytes: bytes::Bytes },
    Exited { agent: usize, code: u32 },
}

pub async fn run(opts: AppOptions) -> Result<()> {
    if opts.agents.is_empty() {
        anyhow::bail!("at least one --profile is required for `opshub tui`");
    }

    let mut agents = opts.agents;
    let mut state = AppState {
        agents: agents
            .iter()
            .map(|a| AgentView {
                label: a.profile.id.clone(),
                buffer: LineBuffer::new(SCROLLBACK_LINES),
                exit_code: None,
            })
            .collect(),
        selected: 0,
        status: match opts.db_label {
            Some(p) => format!("db={p}"),
            None => String::new(),
        },
    };

    // Fan each agent's broadcast into the app loop.
    let (tx, mut rx) = mpsc::channel::<AppMsg>(1024);
    for (idx, agent) in agents.iter().enumerate() {
        let mut sub = agent.subscribe();
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(RunnerEvent::Output(bytes)) => {
                        if tx.send(AppMsg::Bytes { agent: idx, bytes }).await.is_err() {
                            return;
                        }
                    }
                    Ok(RunnerEvent::Exited(code)) => {
                        let _ = tx.send(AppMsg::Exited { agent: idx, code }).await;
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }
    drop(tx);

    let mut terminal = init_terminal().context("init terminal")?;
    let mut events = EventStream::new();

    let initial = terminal.size().context("terminal size")?;
    resize_agents_for(&mut agents, initial.width, initial.height);

    let mut tick = tokio::time::interval(Duration::from_millis(50));

    let outcome: Result<()> = loop {
        terminal
            .draw(|f| ui::render(f, &state))
            .context("draw frame")?;

        tokio::select! {
            _ = tick.tick() => {}
            msg = rx.recv() => match msg {
                Some(AppMsg::Bytes { agent, bytes }) => {
                    if let Some(view) = state.agents.get_mut(agent) {
                        view.buffer.push_bytes(&bytes);
                    }
                }
                Some(AppMsg::Exited { agent, code }) => {
                    if let Some(view) = state.agents.get_mut(agent) {
                        view.exit_code = Some(code);
                    }
                    if state.agents.iter().all(|v| v.exit_code.is_some()) {
                        state.status = "all agents exited — press Ctrl-Q to close".into();
                    }
                }
                None => {}
            },
            maybe_ev = events.next() => match maybe_ev {
                Some(Ok(Event::Key(key))) => {
                    if let Some(res) = handle_key(&mut state, &mut agents, key) {
                        break res;
                    }
                }
                Some(Ok(Event::Resize(cols, rows))) => {
                    resize_agents_for(&mut agents, cols, rows);
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    tracing::warn!(error = %e, "event stream error");
                    break Ok(());
                }
                None => break Ok(()),
            }
        }
    };

    restore_terminal(&mut terminal).ok();
    outcome
}

fn handle_key(
    state: &mut AppState,
    agents: &mut [RunningAgent],
    key: KeyEvent,
) -> Option<Result<()>> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if ctrl && matches!(key.code, KeyCode::Char('q')) {
        return Some(Ok(()));
    }
    if matches!(key.code, KeyCode::Tab) && !ctrl {
        if !state.agents.is_empty() {
            state.selected = (state.selected + 1) % state.agents.len();
        }
        return None;
    }
    if matches!(key.code, KeyCode::BackTab) {
        if !state.agents.is_empty() {
            state.selected = (state.selected + state.agents.len() - 1) % state.agents.len();
        }
        return None;
    }

    if let Some(agent) = agents.get(state.selected) {
        if let Some(bytes) = encode_key(key) {
            if let Err(e) = agent.write_input(&bytes) {
                state.status = format!("write_input error: {e}");
            }
        }
    }
    None
}

fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_alphabetic() {
                    let byte = (lower as u8) - b'a' + 1;
                    return Some(vec![byte]);
                }
                None
            } else {
                let mut buf = [0u8; 4];
                Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
            }
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        _ => None,
    }
}

fn resize_agents_for(agents: &mut [RunningAgent], screen_cols: u16, screen_rows: u16) {
    // Reserve the same chrome the UI reserves: 1 row header + 1 row footer,
    // plus the block border around each cell.
    let body_rows = screen_rows.saturating_sub(2);
    let cells = grid::tile(
        ratatui::layout::Rect::new(0, 0, screen_cols, body_rows),
        agents.len(),
    );
    for (agent, cell) in agents.iter_mut().zip(cells.iter()) {
        let cols = cell.width.saturating_sub(2).max(2);
        let rows = cell.height.saturating_sub(2).max(2);
        if let Err(e) = agent.resize(WinSize { cols, rows }) {
            tracing::warn!(error = %e, agent = %agent.profile.id, "pty resize failed");
        }
    }
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("Terminal::new")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();
    Ok(())
}
