//! ratatui-based N×M agent grid.
//!
//! The TUI subscribes to each `RunningAgent`'s broadcast channel, ANSI-strips
//! the bytes into a line buffer, and renders every agent in its own cell of a
//! near-square grid. Keyboard input is routed to the selected pane (Tab moves
//! focus, Ctrl-Q quits).

pub mod app;
pub mod buffer;
pub mod grid;
pub mod ui;

pub use app::{run, AppOptions};
