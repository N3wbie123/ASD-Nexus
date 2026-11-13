//! ASD-Nexus monochrome ratatui TUI + scriptable CLI.

pub mod app;
pub mod cli;
pub mod exec;
pub mod textarea;
pub mod theme;
pub mod timeline;
pub mod tui;
pub mod ui;

pub use cli::{run, Cli, Command};
