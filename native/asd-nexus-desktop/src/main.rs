//! asd-nexus-desktop entry point: owns the native window and the sidecar
//! lifecycle. The sidecar (`node --no-warnings src/server/index.ts --port 0`)
//! is spawned in the background when `--server-url` is absent and is always
//! terminated (SidecarManager Drop/shutdown) after the window closes.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use asd_nexus_client::{SidecarConfig, SidecarManager};
use asd_nexus_desktop::app::{clean_root, ConnState, NexusApp, RunOptions};
use asd_nexus_desktop::backend::Reply;
use asd_nexus_desktop::cli::{CliArgs, USAGE};
use asd_nexus_desktop::state::Tab;
use asd_nexus_desktop::{fonts, theme};

type SidecarSlot = Arc<Mutex<Option<SidecarManager>>>;

fn sidecar_config(args: &CliArgs) -> SidecarConfig {
    let mut cfg = SidecarConfig::new(args.effective_project_root());
    if let Some(url) = &args.server_url {
        cfg = cfg.with_external_url(url.clone());
    }
    if let Some(t) = &args.token {
        cfg = cfg.with_token(t.clone());
    }
    if let Some(h) = &args.home {
        cfg = cfg.with_home(h);
    }
    if let Some(n) = &args.node_bin {
        cfg = cfg.with_node_bin(n);
    }
    if let Some(s) = &args.server_script {
        cfg = cfg.with_server_script(s);
    }
    cfg
}

/// Start (or attach to) the sidecar on a background thread; the manager is
/// parked in `slot` so `main` can drop it after the event loop exits.
fn connect_async(
    args: &CliArgs,
    tx: std::sync::mpsc::Sender<Reply>,
    slot: SidecarSlot,
    repaint: Option<egui::Context>,
) -> JoinHandle<()> {
    let cfg = sidecar_config(args);
    std::thread::Builder::new()
        .name("nexus-sidecar-start".into())
        .spawn(move || {
            let reply = match SidecarManager::start(cfg) {
                Ok(mgr) => {
                    let info = (mgr.base_url().to_string(), mgr.token().map(str::to_string));
                    *slot.lock().unwrap() = Some(mgr);
                    Ok(info)
                }
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(Reply::Connected(reply));
            if let Some(ctx) = repaint {
                ctx.request_repaint();
            }
        })
        .expect("spawn sidecar start thread")
}

fn shutdown_sidecar(slot: &SidecarSlot, starter: Option<JoinHandle<()>>) {
    if let Some(h) = starter {
        let _ = h.join();
    }
    if let Some(mut mgr) = slot.lock().unwrap().take() {
        let _ = mgr.shutdown();
    }
}

/// `--verify`: headless end-to-end self check (no window).
fn run_verify(args: &CliArgs) -> i32 {
    let slot: SidecarSlot = Arc::new(Mutex::new(None));
    let mut app = NexusApp::new(args.effective_project_root(), RunOptions::default());
    app.conn = ConnState::Connecting;
    // No live polling in verify mode, so the job queue can settle.
    app.subagents.auto_refresh = false;
    let starter = connect_async(args, app.backend.sender(), slot.clone(), None);

    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let (defs, font) = fonts::build_font_definitions(&fonts::default_candidates());
    ctx.set_fonts(defs);
    app.font_status = fonts::describe(font.as_deref());

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut frames = 0u32;
    loop {
        for tab in Tab::ALL {
            app.tab = tab;
            let _ = ctx.run(egui::RawInput::default(), |c| app.ui(c));
            frames += 1;
        }
        // Connected is handled inside `ui()` which immediately submits the
        // bootstrap jobs, so `in_flight == 0` afterwards means all replied.
        let settled = matches!(app.conn, ConnState::Connected(_)) && app.backend.in_flight == 0;
        let failed = matches!(app.conn, ConnState::Failed(_));
        if settled || failed || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let visuals_gray = theme::debug_colors(&format!("{:?}", ctx.style().visuals))
        .iter()
        .all(|[r, g, b, _]| r == g && g == b);
    let ok = app.health.as_ref().map(|h| h.ok).unwrap_or(false) && visuals_gray;
    let tabs: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
    let report = serde_json::json!({
        "ok": ok,
        "projectRoot": app.project_root,
        "connection": format!("{:?}", app.conn),
        "health": app.health.as_ref().map(|h| serde_json::json!({"version": h.version, "pid": h.pid})),
        "spawnedSidecar": args.spawns_sidecar(),
        "projects": app.projects.len(),
        "sessions": app.sessions.len(),
        "boardTasks": app.board.board.as_ref().map(|b| b.tasks.len()),
        "planLoaded": app.plan.loaded,
        "routingLoaded": app.routing.loaded,
        "modelHarnesses": app.chat.picker.models.len(),
        "tabsRendered": tabs,
        "frames": frames,
        "monochromeVisuals": visuals_gray,
        "font": app.font_status,
        "status": app.status,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    drop(app);
    shutdown_sidecar(&slot, Some(starter));
    if ok {
        0
    } else {
        1
    }
}

fn main() {
    let args = match CliArgs::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if args.help {
        print!("{USAGE}");
        return;
    }
    if args.verify {
        std::process::exit(run_verify(&args));
    }

    let slot: SidecarSlot = Arc::new(Mutex::new(None));
    let starter: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let run = RunOptions {
        exit_after: args.exit_after_secs.map(Duration::from_secs_f64),
        screenshot: args.screenshot.clone(),
    };
    let title = format!("ASD Nexus — {}", clean_root(&args.effective_project_root()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };

    let result = {
        let slot = slot.clone();
        let starter = starter.clone();
        let args = args.clone();
        eframe::run_native(
            "asd-nexus-desktop",
            options,
            Box::new(move |cc| {
                let mut app = NexusApp::new(args.effective_project_root(), run);
                app.setup_context(&cc.egui_ctx);
                app.conn = ConnState::Connecting;
                let h = connect_async(&args, app.backend.sender(), slot, Some(cc.egui_ctx.clone()));
                *starter.lock().unwrap() = Some(h);
                Ok(Box::new(app))
            }),
        )
    };

    // Window closed: guarantee sidecar termination (no orphaned node.exe).
    let handle = starter.lock().unwrap().take();
    shutdown_sidecar(&slot, handle);
    if let Err(e) = result {
        eprintln!("asd-nexus-desktop: {e}");
        std::process::exit(1);
    }
}
