//! OS Terminal Launcher Helper.
//!
//! Spawns an external terminal process from `TerminalOpenResponse` on Windows
//! (Windows Terminal, PowerShell, CMD) or Unix shell.

use std::path::Path;
use std::process::{Child, Command};

use crate::error::{NexusError, Result};
use crate::models::{TerminalKind, TerminalOpenResponse};

/// Spawns an external interactive terminal process to resume a session.
pub fn launch_terminal_spec(
    spec: &TerminalOpenResponse,
    preferred: Option<TerminalKind>,
) -> Result<Child> {
    let command_str = spec
        .command
        .as_deref()
        .or_else(|| spec.cli_string.as_deref())
        .ok_or_else(|| NexusError::TerminalError("No command or cliString in TerminalOpenResponse".into()))?;

    let args = spec.args.as_deref().unwrap_or(&[]);
    let cwd = spec
        .cwd
        .as_deref()
        .unwrap_or(".");

    #[cfg(windows)]
    {
        launch_windows_terminal(command_str, args, cwd, preferred)
    }

    #[cfg(not(windows))]
    {
        launch_unix_terminal(command_str, args, cwd)
    }
}

#[cfg(windows)]
fn launch_windows_terminal(
    command_str: &str,
    args: &[String],
    cwd: &str,
    preferred: Option<TerminalKind>,
) -> Result<Child> {
    let kind = preferred.unwrap_or(TerminalKind::Powershell);

    // Build the command line string to run inside the terminal
    let full_inner = if args.is_empty() {
        command_str.to_string()
    } else {
        format!("{} {}", command_str, args.join(" "))
    };

    match kind {
        TerminalKind::WindowsTerminal => {
            // Try wt.exe first, fallback to powershell if wt is not installed
            let mut cmd = Command::new("wt.exe");
            cmd.arg("-d");
            cmd.arg(cwd);
            cmd.arg("powershell.exe");
            cmd.arg("-NoExit");
            cmd.arg("-Command");
            cmd.arg(&full_inner);

            match cmd.spawn() {
                Ok(child) => Ok(child),
                Err(_) => {
                    // Fallback to PowerShell
                    launch_powershell(&full_inner, cwd)
                }
            }
        }
        TerminalKind::Powershell => launch_powershell(&full_inner, cwd),
        TerminalKind::Cmd => {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/K");
            cmd.arg(format!("cd /d \"{}\" && {}", cwd, full_inner));
            if Path::new(cwd).exists() {
                cmd.current_dir(cwd);
            }
            cmd.spawn().map_err(|e| NexusError::TerminalError(format!("Failed to spawn cmd.exe: {e}")))
        }
    }
}

#[cfg(windows)]
fn launch_powershell(inner_command: &str, cwd: &str) -> Result<Child> {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg("-NoExit");
    cmd.arg("-Command");
    cmd.arg(format!("Set-Location -LiteralPath '{}'; {}", cwd, inner_command));
    if Path::new(cwd).exists() {
        cmd.current_dir(cwd);
    }
    cmd.spawn().map_err(|e| NexusError::TerminalError(format!("Failed to spawn powershell.exe: {e}")))
}

#[cfg(not(windows))]
fn launch_unix_terminal(command_str: &str, args: &[String], cwd: &str) -> Result<Child> {
    let full = if args.is_empty() {
        command_str.to_string()
    } else {
        format!("{} {}", command_str, args.join(" "))
    };

    let mut cmd = Command::new("sh");
    cmd.arg("-c");
    cmd.arg(&full);
    if Path::new(cwd).exists() {
        cmd.current_dir(cwd);
    }
    cmd.spawn().map_err(|e| NexusError::TerminalError(format!("Failed to spawn shell: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_open_response_parsing() {
        let resp = TerminalOpenResponse {
            success: true,
            command: Some("claude".into()),
            args: Some(vec!["--resume".into(), "session_123".into()]),
            cli_string: Some("claude --resume session_123".into()),
            cwd: Some(".".into()),
            launched: Some(false),
            error: None,
        };

        assert!(resp.success);
        assert_eq!(resp.command.as_deref(), Some("claude"));
        assert_eq!(resp.args.as_ref().unwrap().len(), 2);
    }
}
