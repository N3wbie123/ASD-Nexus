//! Terminal lifecycle (raw mode + alternate screen + mouse capture with Drop/panic cleanup)
//! and event loop.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use asd_nexus_client::NexusClient;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;
use crate::cli::Cli;
use crate::{exec, ui};

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, cursor::Show);
}

/// Restores the terminal on Drop (normal exit, `?` error, or unwinding panic).
pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            prev(info);
        }));
        enable_raw_mode()?;
        let mut out = io::stdout();
        if let Err(e) = execute!(out, EnterAlternateScreen, EnableMouseCapture) {
            restore();
            return Err(e.into());
        }
        let terminal = Terminal::new(CrosstermBackend::new(out))?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

pub fn run_tui(cli: &Cli, client: &NexusClient) -> Result<()> {
    let mut app = App::new(cli.project_root_string(), cli.harness.clone(), cli.model.clone());
    app.cli_session = cli.session.clone();
    app.current_asd = cli.session.clone();
    app.server_label = client.base_url().to_string();

    let mut guard = TerminalGuard::enter()?;
    loop {
        app.drain_stream();
        app.tick(Instant::now());
        exec::run_pending(&mut app, client);
        guard.terminal.draw(|f| ui::draw(f, &app))?;
        if app.should_quit {
            break;
        }
        if event::poll(Duration::from_millis(80))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.handle_key(k),
                Event::Mouse(m) => app.handle_mouse(m),
                _ => {}
            }
        }
    }
    // Flush e.g. CancelTurn queued by quit.
    exec::run_pending(&mut app, client);
    Ok(())
}
