//! ASD-Nexus Desktop: pure native (eframe/egui + rfd, 0% WebView) monochrome GUI
//! for the ASD-Nexus orchestrator sidecar.
//!
//! The crate is split into a headless-testable library (state, reducers, theme,
//! fonts, CLI parsing, backend job runner, egui views) and a thin `main.rs`
//! that owns the window and the sidecar lifecycle.

pub mod app;
pub mod backend;
pub mod cli;
pub mod fonts;
pub mod screenshot;
pub mod state;
pub mod theme;
pub mod timeline;

pub use app::NexusApp;
pub use cli::CliArgs;
