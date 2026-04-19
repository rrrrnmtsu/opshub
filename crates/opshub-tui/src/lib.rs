//! ratatui-based N×M agent grid.
//!
//! The TUI subscribes to each `RunningAgent`'s broadcast channel, feeds raw
//! PTY bytes through a `vt100::Parser`-backed screen, and renders every agent
//! in its own cell of a near-square grid. Keyboard input is routed to the
//! selected pane (Tab moves focus, Ctrl-Q quits, Ctrl-M toggles the $ /
//! tok/s header).

pub mod app;
pub mod buffer;
pub mod grid;
pub mod screen;
pub mod ui;

pub use app::{run, AppOptions};
