//! ASD-Nexus Client: Rust SDK for the ASD-Nexus Orchestrator Server.
//!
//! Provides sidecar process lifecycle management, complete protocol DTOs,
//! typed REST API client, SSE turn streaming, and OS terminal integration.

pub mod client;
pub mod error;
pub mod models;
pub mod sidecar;
pub mod sse;
pub mod terminal;

pub use client::{NexusClient, TurnStreamHandle};
pub use error::{NexusError, Result};
pub use models::*;
pub use sidecar::{find_server_script, SidecarConfig, SidecarManager};
pub use sse::{SseParser, StreamMessage};
pub use terminal::launch_terminal_spec;
