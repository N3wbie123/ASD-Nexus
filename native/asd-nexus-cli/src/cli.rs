//! clap CLI definition, sidecar connection and scriptable subcommands.

use std::path::PathBuf;

use anyhow::{Context, Result};
use asd_nexus_client::*;
use clap::{Parser, Subcommand};

use crate::app::HARNESSES;

#[derive(Debug, Parser, PartialEq)]
#[command(name = "asd-nexus", version, about = "ASD-Nexus monochrome TUI / CLI")]
pub struct Cli {
    /// Project root (defaults to current directory).
    #[arg(long, global = true)]
    pub project_root: Option<PathBuf>,
    /// Connect to an already-running sidecar instead of spawning one.
    #[arg(long, global = true)]
    pub server_url: Option<String>,
    /// Bearer token for the sidecar.
    #[arg(long, global = true)]
    pub token: Option<String>,
    /// Harness: claude-code | codex | opencode | antigravity | cursor.
    #[arg(long, global = true, default_value = "claude-code", value_parser = HARNESSES)]
    pub harness: String,
    /// Model override for the selected harness.
    #[arg(long, global = true)]
    pub model: Option<String>,
    /// ASD session id.
    #[arg(long, global = true)]
    pub session: Option<String>,
    /// Emit JSON (scriptable mode).
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Command {
    /// Check sidecar health.
    Health,
    /// List discovered 5-harness sessions.
    Sessions {
        /// Maximum number of sessions.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show the progress board.
    Board,
}

impl Cli {
    pub fn project_root_string(&self) -> String {
        self.project_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    }
}

/// Live connection; owns the sidecar (if spawned) so Drop shuts it down.
pub struct Connection {
    pub client: NexusClient,
    pub sidecar: Option<SidecarManager>,
}

pub fn connect(cli: &Cli) -> Result<Connection> {
    if let Some(url) = &cli.server_url {
        return Ok(Connection { client: NexusClient::new(url.clone(), cli.token.clone()), sidecar: None });
    }
    let mut cfg = SidecarConfig::new(cli.project_root_string());
    if let Some(t) = &cli.token {
        cfg = cfg.with_token(t.clone());
    }
    let sidecar = SidecarManager::start(cfg).context("failed to start ASD-Nexus sidecar (node src/server/index.ts --port 0)")?;
    Ok(Connection { client: sidecar.create_client(), sidecar: Some(sidecar) })
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

pub fn run_command(cli: &Cli, cmd: &Command, client: &NexusClient) -> Result<()> {
    let root = cli.project_root_string();
    match cmd {
        Command::Health => {
            let h = client.health()?;
            if cli.json {
                return print_json(&h);
            }
            println!("ok={} version={} pid={} root={}", h.ok, h.version, h.pid, h.default_project_root);
        }
        Command::Sessions { limit } => {
            let r = client.sessions(&GetSessionsRequest {
                project_root: Some(root),
                limit: *limit,
                sync_local_transcripts: None,
            })?;
            if cli.json {
                return print_json(&r);
            }
            for s in &r.sessions {
                let fg = s.asd_session_id.is_some() && s.asd_session_id == r.active_session_id;
                println!(
                    "{}{} {:<12} {:<40} {:<28} {}",
                    if fg { "*" } else { " " },
                    if s.is_promoted_to_asd { "A" } else { "-" },
                    s.harness,
                    s.harness_session_id,
                    s.asd_session_id.as_deref().unwrap_or("-"),
                    s.summary.as_deref().unwrap_or("").lines().next().unwrap_or("")
                );
            }
        }
        Command::Board => {
            let b = client.board(&GetBoardRequest { project_root: Some(root), session_id: cli.session.clone() })?;
            if cli.json {
                return print_json(&b);
            }
            println!(
                "session {} · phase {} · progress {}",
                b.session_id.as_deref().unwrap_or("-"),
                b.phase.as_deref().unwrap_or("-"),
                b.progress.as_deref().unwrap_or("-")
            );
            for t in &b.tasks {
                println!("[{:<10}] {:<8} {} ({})", t.status, t.id, t.subject, t.assignee_role.as_deref().unwrap_or("-"));
            }
        }
    }
    Ok(())
}

pub fn run(cli: Cli) -> Result<()> {
    let conn = connect(&cli)?;
    match &cli.command {
        Some(cmd) => run_command(&cli, cmd, &conn.client),
        None => crate::tui::run_tui(&cli, &conn.client),
    }
    // `conn` drops here → SidecarManager::drop shuts the child down.
}
