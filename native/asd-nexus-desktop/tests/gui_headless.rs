//! Headless egui integration tests for the desktop UI:
//! - `Chat` ↔ `Dashboard` primary mode split (`Ctrl+1` / `Ctrl+2`) and all 5 Dashboard subtabs
//! - `ThemeMode::DarkColor` default rendering + `ThemeMode::Monochrome` transition where every
//!   tessellated vertex passes `theme::is_grayscale`
//! - Inline `/` slash popup (Nexus built-in + active harness commands) and `@` workspace file popup
//! - Automatic new session creation (`CreateSession` + `HarnessCommands`) on manual harness change
//!   (and preservation when selecting an existing session)
//! - SSE 4-category event reduction into the chat timeline, PLAN.md conflict flows, ModelPicker merge,
//!   and clipboard copy.

use asd_nexus_client::*;
use asd_nexus_desktop::app::{is_thinking_open, thinking_card_rect, ConnState, NexusApp, PaletteAction, RunOptions};
use asd_nexus_desktop::backend::{Reply, Request, SavePlanOutcome};
use asd_nexus_desktop::state::{PrimaryMode, Tab};
use asd_nexus_desktop::theme::{self, ThemeMode};
use asd_nexus_desktop::timeline::EntryKind;

fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    ctx
}

fn frame(ctx: &egui::Context, app: &mut NexusApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
        events,
        ..Default::default()
    };
    ctx.run(input, |c| app.ui(c))
}

fn ev(json: &str) -> StreamMessage {
    StreamMessage::Event(Box::new(serde_json::from_str(json).unwrap()))
}

fn plan(content: &str, sha: &str, mtime: f64) -> GetPlanResponse {
    GetPlanResponse {
        session_id: Some("asd-1".into()),
        plan_md_path: "C:/p/.asd/PLAN.md".into(),
        exists: true,
        content: content.into(),
        mtime_ms: Some(mtime),
        sha256: Some(sha.into()),
    }
}

/// Fill the app with representative data for Chat and every Dashboard subtab.
fn populate(app: &mut NexusApp) {
    app.handle_reply(Reply::Projects(Ok(ListProjectsResponse {
        projects: vec![ProjectSummaryItem {
            project_root: "C:/work/한글프로젝트".into(),
            project_hash: "h".into(),
            session_count: Some(3),
            last_active_at: None,
        }],
    })));
    app.handle_reply(Reply::Sessions(Ok(GetSessionsResponse {
        sessions: vec![
            DiscoveredSessionItem {
                harness: "claude-code".into(),
                harness_session_id: "cc-111".into(),
                project_root: app.project_root.clone(),
                updated_at: 1.0,
                model: Some("sonnet".into()),
                summary: Some("세션 요약 summary".into()),
                transcript_path: None,
                asd_session_id: Some("asd-1".into()),
                asd_phase: Some("running".into()),
                is_promoted_to_asd: true,
                auto_approve: Some(true),
            },
            DiscoveredSessionItem {
                harness: "codex".into(),
                harness_session_id: "cx-222".into(),
                project_root: app.project_root.clone(),
                updated_at: 2.0,
                model: None,
                summary: None,
                transcript_path: None,
                asd_session_id: None,
                asd_phase: None,
                is_promoted_to_asd: false,
                auto_approve: None,
            },
        ],
        active_session_id: Some("asd-1".into()),
    })));
    app.handle_reply(Reply::Models(Ok(GetModelsResponse {
        models: [
            ("claude-code".to_string(), vec!["sonnet".to_string(), "opus".to_string()]),
            ("antigravity".to_string(), vec!["gemini-pro".to_string()]),
        ]
        .into_iter()
        .collect(),
        cache_path: None,
    })));
    app.handle_reply(Reply::HarnessCommands(Ok(GetHarnessCommandsResponse {
        commands: vec![
            HarnessCommandItem {
                name: "compact".into(),
                description: "Compact conversation context".into(),
                harness: "claude-code".into(),
                source: HarnessCommandSource::Builtin,
                kind: HarnessCommandKind::Command,
                path: None,
            },
            HarnessCommandItem {
                name: "asd-plan".into(),
                description: "Run ASD planning workflow".into(),
                harness: "claude-code".into(),
                source: HarnessCommandSource::Global,
                kind: HarnessCommandKind::Skill,
                path: Some("~/.claude/skills/asd-plan/SKILL.md".into()),
            },
        ],
    })));
    app.handle_reply(Reply::FsList(Ok(FsListResponse {
        project_root: app.project_root.clone(),
        mode: FsListMode::Files,
        base_dir: app.project_root.clone(),
        entries: vec![
            FsListEntry {
                path: "native/asd-nexus-desktop/src/app.rs".into(),
                name: "app.rs".into(),
                is_dir: false,
            },
            FsListEntry {
                path: "native/asd-nexus-desktop/src/theme.rs".into(),
                name: "theme.rs".into(),
                is_dir: false,
            },
        ],
        truncated: Some(false),
    })));
    app.handle_reply(Reply::Board(Ok(GetBoardResponse {
        session_id: Some("asd-1".into()),
        phase: Some("running".into()),
        progress: Some("1/4".into()),
        tasks: ["pending", "in_progress", "review", "done", "failed"]
            .iter()
            .enumerate()
            .map(|(i, s)| BoardTaskItem {
                id: format!("T{i}"),
                subject: format!("task {s}"),
                status: s.to_string(),
                assignee_role: Some("worker".into()),
                difficulty: Some("medium".into()),
                turn: Some(1),
                review_verdict: None,
                summary: None,
            })
            .collect(),
    })));
    app.handle_reply(Reply::Plan(Ok(plan("# PLAN\n- [ ] T1", "sha-a", 100.0))));
    app.handle_reply(Reply::Diff(Ok(WorktreeDiffResponse {
        task_id: Some("T1".into()),
        worktree_path: Some(".asd/wt/1-T1".into()),
        mode: Some("working".into()),
        base_ref: Some("HEAD".into()),
        files: vec![WorktreeDiffFile {
            path: "src/a.rs".into(),
            status: Some("M".into()),
            additions: Some(2),
            deletions: Some(1),
        }],
        status_short: Some(" M src/a.rs".into()),
        diff_text: Some(
            "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,3 @@\n ctx\n-old\n+new\n+more"
                .into(),
        ),
        error: None,
    })));
    app.handle_reply(Reply::Subagents(Ok(GetSubagentsResponse {
        subagents: vec![SubagentRecordItem {
            sub_session_id: "sub-abcdef0123456789".into(),
            parent_session_id: "asd-1".into(),
            task_id: Some("T1".into()),
            role: Some("worker".into()),
            harness: Some("codex".into()),
            model: Some("gpt".into()),
            status: "running".into(),
            start_time: None,
            end_time: None,
            exit_code: None,
            log_path: None,
            prompt_path: None,
        }],
    })));
    app.subagents.selected = Some("sub-abcdef0123456789".into());
    app.handle_reply(Reply::SubagentLogs(Ok(SubagentLogTailResponse {
        sub_session_id: "sub-abcdef0123456789".into(),
        lines: vec!["line 1".into(), "로그 2".into()],
        truncated: None,
        error: None,
    })));
    app.handle_reply(Reply::Routing(Ok(GetRoutingResponse {
        project_root: app.project_root.clone(),
        routing: serde_json::json!({
            "lightSubagents": ["scout"],
            "agents": {"worker": {"harness": "codex", "model": "gpt-5"}},
            "categories": {"quick": {"harness": "opencode"}}
        }),
    })));
    app.handle_reply(Reply::ResumeCommand(Ok(GetResumeCommandResponse {
        success: true,
        harness: "claude-code".into(),
        harness_session_id: Some("cc-111".into()),
        command: "claude".into(),
        args: vec!["--resume".into(), "cc-111".into()],
        cli_string: "claude --resume cc-111".into(),
        cwd: "C:/p".into(),
        error: None,
    })));
    app.chat.turn_seq = Some(7);
    app.chat.timeline.begin_turn("안녕");
    for m in [
        ev(r#"{"kind":"thinking","thinking":"hmm"}"#),
        ev(r#"{"kind":"tool_use","toolUse":{"name":"read_file","id":"c1","input":"{}"}}"#),
        ev(r#"{"kind":"tool_result","toolResult":{"toolUseId":"c1","output":"boom","isError":true}}"#),
        ev(r####"{"kind":"text","text":"### Result\n- item\n```rust\nlet x = 1;\n```\ndone"}"####),
        ev(r#"{"kind":"error","error":"oops"}"#),
    ] {
        app.handle_reply(Reply::Stream { seq: 7, msg: m });
    }
}

fn assert_grayscale_output(ctx: &egui::Context, out: egui::FullOutput) -> usize {
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    let mut n = 0;
    for p in prims {
        if let egui::epaint::Primitive::Mesh(mesh) = p.primitive {
            for v in &mesh.vertices {
                let c = v.color;
                assert!(theme::is_grayscale(c), "non-grayscale vertex color {c:?}");
                n += 1;
            }
        }
    }
    n
}

fn count_colored_vertices(ctx: &egui::Context, out: egui::FullOutput) -> (usize, usize) {
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    let mut total = 0;
    let mut colored = 0;
    for p in prims {
        if let egui::epaint::Primitive::Mesh(mesh) = p.primitive {
            for v in &mesh.vertices {
                total += 1;
                if !theme::is_grayscale(v.color) {
                    colored += 1;
                }
            }
        }
    }
    (total, colored)
}

#[test]
fn dark_color_default_and_monochrome_switch_with_strict_grayscale_vertices() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/work/proj", RunOptions::default());
    assert_eq!(app.theme_mode, ThemeMode::DarkColor, "DarkColor must be the default theme");
    populate(&mut app);

    // 1. Render in default DarkColor mode: must paint colored slate/accent vertices.
    for tab in Tab::ALL {
        app.tab = tab;
        let _ = frame(&ctx, &mut app, vec![]);
        let out = frame(&ctx, &mut app, vec![]);
        let (total, colored) = count_colored_vertices(&ctx, out);
        assert!(total > 100, "{tab:?} painted too few vertices in DarkColor ({total})");
        assert!(colored > 0, "{tab:?} should have non-grayscale vertices in DarkColor");
    }

    // 2. Switch to ThemeMode::Monochrome: every tessellated vertex across all tabs must be grayscale.
    app.set_theme_mode(&ctx, ThemeMode::Monochrome);
    assert_eq!(app.theme_mode, ThemeMode::Monochrome);
    for tab in Tab::ALL {
        app.tab = tab;
        let _ = frame(&ctx, &mut app, vec![]);
        let out = frame(&ctx, &mut app, vec![]);
        let vertices = assert_grayscale_output(&ctx, out);
        assert!(vertices > 100, "{tab:?} painted too little ({vertices} vertices)");
    }
}

#[test]
fn chat_and_dashboard_mode_separation_and_subtabs() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);

    let key = |k: egui::Key| egui::Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };

    // Starts in Chat mode
    let _ = frame(&ctx, &mut app, vec![]);
    assert_eq!(app.mode, PrimaryMode::Chat);
    assert_eq!(app.tab, Tab::Chat);

    // Ctrl+2 switches to Dashboard mode (defaults to PLAN & Board subtab)
    let _ = frame(&ctx, &mut app, vec![key(egui::Key::Num2)]);
    assert_eq!(app.mode, PrimaryMode::Dashboard);
    assert_eq!(app.dashboard_tab, Tab::Board);
    assert_eq!(app.tab, Tab::Board);

    // Render all 5 Dashboard subtabs cleanly
    for subtab in Tab::DASHBOARD {
        app.select_dashboard_tab(subtab);
        let out = frame(&ctx, &mut app, vec![]);
        let (total, _) = count_colored_vertices(&ctx, out);
        assert!(total > 100, "Dashboard subtab {subtab:?} failed to render");
        assert_eq!(app.mode, PrimaryMode::Dashboard);
        assert_eq!(app.tab, subtab);
        assert_eq!(app.dashboard_tab, subtab);
    }

    // Ctrl+1 returns to pure Chat mode
    let _ = frame(&ctx, &mut app, vec![key(egui::Key::Num1)]);
    assert_eq!(app.mode, PrimaryMode::Chat);
    assert_eq!(app.tab, Tab::Chat);
    // Dashboard remembers the last active subtab when switching back with Ctrl+2
    let _ = frame(&ctx, &mut app, vec![key(egui::Key::Num2)]);
    assert_eq!(app.mode, PrimaryMode::Dashboard);
    assert_eq!(app.tab, Tab::Terminal);
}

#[test]
fn slash_and_at_autocomplete_popups_render_and_apply() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);
    app.set_mode(PrimaryMode::Chat);

    // 1. Type `/` in composer: popup lists both `[nexus]` built-ins and `[claude-code]` harness commands.
    app.chat.composer = "/".into();
    let slash_items = app.current_slash_items();
    assert!(slash_items.iter().any(|i| i.is_nexus && i.name == "cd" && i.badge == "nexus"));
    assert!(slash_items.iter().any(|i| i.is_nexus && i.name == "dashboard" && i.badge == "nexus"));
    assert!(slash_items.iter().any(|i| !i.is_nexus && i.name == "compact" && i.badge == "claude-code"));
    assert!(slash_items.iter().any(|i| !i.is_nexus && i.name == "asd-plan" && i.badge == "claude-code"));

    // Render frame with slash popup open (also verify Monochrome grayscale while popup is open)
    app.set_theme_mode(&ctx, ThemeMode::Monochrome);
    let _ = frame(&ctx, &mut app, vec![]);
    let out = frame(&ctx, &mut app, vec![]);
    assert!(assert_grayscale_output(&ctx, out) > 100);
    app.set_theme_mode(&ctx, ThemeMode::DarkColor);

    // Filter to `/compact` and accept item -> inserts `/compact ` into composer
    app.chat.composer = "/comp".into();
    let filtered = app.current_slash_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "compact");
    assert!(app.accept_slash_item(0, Some(&ctx)));
    assert_eq!(app.chat.composer, "/compact ");

    // Built-in `/dashboard` command switches to Dashboard mode
    app.chat.composer = "/dashboard".into();
    app.send_prompt();
    assert_eq!(app.mode, PrimaryMode::Dashboard);
    assert!(app.chat.composer.is_empty());

    // Switch back to Chat and test `@` workspace file autocomplete popup
    app.set_mode(PrimaryMode::Chat);
    app.chat.composer = "Please review @theme".into();
    let at_items = app.current_at_items();
    assert_eq!(at_items.len(), 1);
    assert_eq!(at_items[0].path, "native/asd-nexus-desktop/src/theme.rs");

    // Render frame with `@` popup open
    let _ = frame(&ctx, &mut app, vec![]);
    assert!(app.accept_at_item(0));
    assert_eq!(app.chat.composer, "Please review @native/asd-nexus-desktop/src/theme.rs ");

    // Pressing Enter while `@` popup is open completes the item rather than sending raw `@theme`
    app.chat.turn_seq = None;
    app.backend.submitted.clear();
    app.status.clear();
    app.chat.composer = "Inspect @theme".into();
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("chat-composer")));
    let _ = frame(&ctx, &mut app, vec![]);
    let enter = || egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = frame(&ctx, &mut app, vec![enter()]);
    assert_eq!(app.chat.composer, "Inspect @native/asd-nexus-desktop/src/theme.rs ");
    assert!(app.backend.submitted.is_empty(), "Enter on @ popup must complete, not send");
    assert!(!app.status.contains("not connected"));

    // Pressing Enter while `/` popup is open applies the highlighted slash item instead of sending raw `/dash`
    app.chat.composer = "/dash".into();
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("chat-composer")));
    let _ = frame(&ctx, &mut app, vec![]);
    let _ = frame(&ctx, &mut app, vec![enter()]);
    assert_eq!(app.mode, PrimaryMode::Dashboard);
    assert!(app.chat.composer.is_empty());
}

#[test]
fn manual_harness_change_resets_state_and_locks_on_started_session() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);

    // 1. Initial state is New Session draft: harness CAN be changed freely without creating a session.
    assert!(!app.is_session_started());
    assert!(app.active_session_id.is_none(), "draft must not adopt active_session_id from Reply::Sessions");
    app.backend.submitted.clear();
    app.change_harness_manually(3); // antigravity
    assert_eq!(app.chat.harness(), "antigravity");
    assert!(!app.is_session_started(), "harness change in draft stays in draft");
    assert!(
        !app.backend.submitted.iter().any(|r| matches!(r, Request::CreateSession(_))),
        "harness change in draft must not eagerly emit CreateSession"
    );
    assert!(
        app.backend.submitted.iter().any(|r| matches!(
            r,
            Request::HarnessCommands(req) if req.harness.as_deref() == Some("antigravity")
        )),
        "harness change in draft must refresh harness_commands"
    );

    // 2. Select an existing session from left sidebar: locks harness (`is_session_started() == true`).
    app.backend.submitted.clear();
    app.select_session(0);
    assert!(app.is_session_started());
    assert_eq!(app.chat.harness(), "claude-code");
    assert_eq!(app.active_session_id.as_deref(), Some("asd-1"));
    assert!(app.selected_session.is_some());
    assert!(
        !app.backend.submitted.iter().any(|r| matches!(r, Request::CreateSession(_))),
        "selecting an existing session must not overwrite with CreateSession"
    );

    // While started, harness switching via API, `/harness`, slash popup, and palette are all blocked.
    app.change_harness_manually(3);
    assert_eq!(app.chat.harness(), "claude-code", "harness must stay locked");
    assert!(app.status.contains("locked"));

    app.chat.composer = "/harness codex".into();
    app.send_prompt();
    assert_eq!(app.chat.harness(), "claude-code", "/harness must be blocked when started");
    assert!(app.status.contains("locked"));

    app.chat.composer = "/".into();
    assert!(
        !app.current_slash_items().iter().any(|i| i.name == "harness"),
        "/harness must be hidden from slash popup when session is started"
    );
    app.chat.composer.clear();

    let palette_entries = app.build_palette_entries();
    assert!(
        !palette_entries.iter().any(|e| matches!(e.action, PaletteAction::SwitchHarness(_))),
        "SwitchHarness must be hidden from Command Palette when session is started"
    );
    assert!(
        !palette_entries
            .iter()
            .any(|e| matches!(e.action, PaletteAction::SelectModel(h_idx, _) if h_idx != app.chat.harness_idx)),
        "Command Palette must only list models for the locked harness"
    );

    // Model selection STILL works on a started session!
    app.execute_palette_action(PaletteAction::SelectModel(0, Some("opus".into())), Some(&ctx));
    assert_eq!(app.chat.picker.selected.as_deref(), Some("opus"));
    assert_eq!(app.chat.harness(), "claude-code");

    // Render started Chat in DarkColor and Monochrome (read-only harness badge, no ComboBox)
    let _ = frame(&ctx, &mut app, vec![]);
    app.set_theme_mode(&ctx, ThemeMode::Monochrome);
    let out = frame(&ctx, &mut app, vec![]);
    assert!(assert_grayscale_output(&ctx, out) > 100);
    app.set_theme_mode(&ctx, ThemeMode::DarkColor);

    // 3. `/new` (or + New Chat) re-enters New Session draft where harness CAN be changed again.
    app.chat.composer = "/new".into();
    app.send_prompt();
    assert!(!app.is_session_started());
    assert!(app.active_session_id.is_none());
    assert!(app.selected_session.is_none());
    assert!(app.chat.timeline.entries.is_empty());

    app.change_harness_manually(3);
    assert_eq!(app.chat.harness(), "antigravity");

    // 4. Sending a prompt on a connected draft creates a fresh session (`session_id: None`) and locks harness.
    app.backend.set_client(Some(NexusClient::new("http://127.0.0.1:9", None)));
    app.chat.turn_seq = None;
    app.backend.submitted.clear();
    app.chat.composer = "hello new session".into();
    app.send_prompt();
    assert!(app.is_session_started(), "sending a prompt must lock the session");
    let has_start_turn = app.backend.submitted.iter().any(|r| match r {
        Request::StartTurn { req, .. } => req.session_id.is_none() && req.harness == "antigravity",
        _ => false,
    });
    assert!(has_start_turn, "first turn from draft must send session_id=None on chosen harness");

    // Simulate CreateSession reply arriving from backend
    app.handle_reply(Reply::CreateSession(Ok(CreateSessionResponse {
        success: true,
        session_id: "asd-new-agy".into(),
        phase: SessionPhase::Planning,
        auto_approve: Some(true),
        error: None,
    })));
    assert_eq!(app.active_session_id.as_deref(), Some("asd-new-agy"));
    assert!(app.is_session_started());
}

#[test]
fn auto_approve_toggle_and_permission_modal_flow() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);
    app.select_session(0);
    assert!(app.chat.auto_approve);

    // 1. Toggling auto_approve on an active session dispatches Request::SetSessionAutoApprove
    app.backend.submitted.clear();
    app.set_auto_approve(false);
    assert!(!app.chat.auto_approve);
    let sent_toggle = app.backend.submitted.iter().any(|r| match r {
        Request::SetSessionAutoApprove(req) => req.session_id == "asd-1" && !req.auto_approve,
        _ => false,
    });
    assert!(sent_toggle, "set_auto_approve must dispatch Request::SetSessionAutoApprove");

    // `/auto-approve on` slash command also updates and dispatches
    app.backend.submitted.clear();
    app.chat.composer = "/auto-approve on".into();
    app.send_prompt();
    assert!(app.chat.auto_approve);
    assert!(app.backend.submitted.iter().any(|r| match r {
        Request::SetSessionAutoApprove(req) => req.session_id == "asd-1" && req.auto_approve,
        _ => false,
    }));

    // 2. Receiving PermissionRequest SSE event populates pending_permissions & renders cleanly in DarkColor + Monochrome
    app.chat.turn_seq = Some(42);
    app.chat.timeline.begin_turn("run bash");
    let perm_ev = ev(
        r#"{
            "kind": "permission_request",
            "turnId": "turn-perm-1",
            "sessionId": "asd-1",
            "permissionRequest": {
                "requestId": "req-99",
                "toolName": "Bash",
                "toolKind": "execute",
                "toolInput": "{\"command\":\"echo hi\"}",
                "locations": ["src/main.rs"],
                "options": [
                    {"optionId": "allow_once", "name": "Allow Once", "kind": "allow_once"},
                    {"optionId": "allow_always", "name": "Allow Always", "kind": "allow_always"},
                    {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"}
                ]
            }
        }"#,
    );
    app.handle_reply(Reply::Stream { seq: 42, msg: perm_ev });
    assert_eq!(app.chat.pending_permissions.len(), 1);
    assert_eq!(app.chat.pending_permissions[0].1.tool_name, "Bash");

    // Render modal in DarkColor
    let _ = frame(&ctx, &mut app, vec![]);
    let out_dark = frame(&ctx, &mut app, vec![]);
    let (total_dark, colored_dark) = count_colored_vertices(&ctx, out_dark);
    assert!(total_dark > 100 && colored_dark > 0);

    // Render modal in strict Monochrome grayscale
    app.set_theme_mode(&ctx, ThemeMode::Monochrome);
    let _ = frame(&ctx, &mut app, vec![]);
    let out_mono = frame(&ctx, &mut app, vec![]);
    assert!(assert_grayscale_output(&ctx, out_mono) > 100);
    app.set_theme_mode(&ctx, ThemeMode::DarkColor);

    // 3. Responding with `allow_always` pops pending_permissions, flips auto_approve=true, and dispatches SubmitTurnPermission
    app.chat.auto_approve = false;
    app.backend.submitted.clear();
    app.respond_to_permission("selected", Some("allow_always".into()));
    assert!(app.chat.pending_permissions.is_empty());
    assert!(app.chat.auto_approve, "allow_always must update auto_approve=true locally");
    let sent_perm = app.backend.submitted.iter().any(|r| match r {
        Request::SubmitTurnPermission(req) => {
            req.turn_id == "turn-perm-1"
                && req.request_id == "req-99"
                && req.outcome == "selected"
                && req.option_id.as_deref() == Some("allow_always")
        }
        _ => false,
    });
    assert!(sent_perm, "respond_to_permission must dispatch Request::SubmitTurnPermission");

    // 4. Toggling auto_approve=false in new_session_draft passes auto_approve: Some(false) on first send_prompt()
    app.start_new_session_draft();
    app.chat.turn_seq = None;
    app.backend.submitted.clear();
    app.set_auto_approve(false);
    assert!(!app.chat.auto_approve);
    assert!(
        !app.backend.submitted.iter().any(|r| matches!(r, Request::SetSessionAutoApprove(_))),
        "toggling auto_approve in draft must not emit SetSessionAutoApprove"
    );
    app.backend.set_client(Some(NexusClient::new("http://127.0.0.1:9", None)));
    app.backend.submitted.clear();
    app.chat.composer = "first draft turn with manual approval".into();
    app.send_prompt();
    let draft_turn_has_manual_approve = app.backend.submitted.iter().any(|r| match r {
        Request::StartTurn { req, auto_approve } => {
            req.session_id.is_none() && *auto_approve == Some(false)
        }
        _ => false,
    });
    assert!(
        draft_turn_has_manual_approve,
        "first send_prompt() in draft with auto_approve=false must submit Request::StartTurn with auto_approve=Some(false)"
    );
}

#[test]
fn command_palette_modal_renders_and_executes_actions() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);

    let ctrl_p = egui::Event::Key {
        key: egui::Key::P,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    let _ = frame(&ctx, &mut app, vec![ctrl_p]);
    assert!(app.palette_open);

    // Render palette modal and verify entries (only current Workspace open action, no multi-project dump)
    let _ = frame(&ctx, &mut app, vec![]);
    let entries = app.build_palette_entries();
    assert_eq!(entries.iter().filter(|e| e.category == "Workspace").count(), 1);
    assert!(!entries.iter().any(|e| e.category == "Project"));
    assert!(entries.iter().any(|e| e.category == "Harness"));
    assert!(entries.iter().any(|e| e.category == "Command"));

    app.execute_palette_action(PaletteAction::SwitchDashboardTab(Tab::Routing), Some(&ctx));
    assert!(!app.palette_open);
    assert_eq!(app.mode, PrimaryMode::Dashboard);
    assert_eq!(app.tab, Tab::Routing);
}

#[test]
fn sse_stream_reduces_into_timeline_and_ignores_stale_turns() {
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.chat.turn_seq = Some(3);
    app.chat.timeline.begin_turn("hello");
    let msgs = [
        ev(r#"{"kind":"status","status":"running","turnId":"turn_1"}"#),
        ev(r#"{"kind":"thinking","thinking":"plan "}"#),
        ev(r#"{"kind":"thinking","thinking":"more"}"#),
        ev(r#"{"kind":"tool_use","toolUse":{"name":"grep","id":"g1","input":"x"}}"#),
        ev(r#"{"kind":"tool_result","toolResult":{"toolUseId":"g1","output":"3 hits"}}"#),
        ev(r#"{"kind":"text","text":"Ans"}"#),
        ev(r#"{"kind":"text","text":"wer"}"#),
        ev(r#"{"kind":"usage","usage":{"inputTokens":10,"outputTokens":5,"totalTokens":15,"costUsd":0.01}}"#),
    ];
    for m in msgs {
        app.handle_reply(Reply::Stream { seq: 3, msg: m });
    }
    // stale stream from an old turn is ignored
    app.handle_reply(Reply::Stream { seq: 2, msg: ev(r#"{"kind":"text","text":"STALE"}"#) });
    let tl = &app.chat.timeline;
    assert!(tl.streaming);
    assert_eq!(tl.turn_id.as_deref(), Some("turn_1"));
    assert_eq!(tl.count(EntryKind::Thinking), 1);
    assert_eq!(tl.count(EntryKind::ToolUse), 1);
    assert_eq!(tl.count(EntryKind::ToolResult), 1);
    assert_eq!(tl.count(EntryKind::Text), 1);
    assert!(tl.entries.iter().all(|e| !e.body.contains("STALE")));
    assert_eq!(tl.entries.last().unwrap().body, "Answer");
    assert!(app.chat.is_streaming());

    app.handle_reply(Reply::Stream { seq: 3, msg: StreamMessage::Done });
    assert!(!app.chat.is_streaming());
    assert!(!app.chat.timeline.streaming);
    assert_eq!(app.chat.timeline.total_usage.total, 15);
}

#[test]
fn cancel_before_handle_defers_and_drops_stream() {
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.chat.turn_seq = Some(1);
    app.chat.timeline.begin_turn("x");
    app.cancel_turn();
    assert!(app.chat.cancel_requested);
    assert!(!app.chat.timeline.streaming);
    app.handle_reply(Reply::Stream { seq: 1, msg: ev(r#"{"kind":"text","text":"late"}"#) });
    assert!(app.chat.timeline.entries.iter().all(|e| e.body != "late"));
    // turn start failure clears state
    app.handle_reply(Reply::TurnStarted { seq: 1, handle: Err("refused".into()) });
    assert!(!app.chat.is_streaming());
    assert!(!app.chat.cancel_requested);
}

#[test]
fn turn_start_error_is_rendered() {
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.chat.turn_seq = Some(9);
    app.chat.timeline.begin_turn("x");
    app.handle_reply(Reply::TurnStarted { seq: 9, handle: Err("HTTP 500".into()) });
    assert_eq!(app.chat.timeline.count(EntryKind::Error), 1);
    assert!(!app.chat.is_streaming());
}

#[test]
fn plan_conflict_flows() {
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.handle_reply(Reply::Plan(Ok(plan("v1", "s1", 1.0))));
    assert!(app.plan.loaded && !app.plan.is_dirty());

    // Edit, then disk changes -> conflict, local edits preserved.
    app.plan.buffer.push_str(" local");
    app.handle_reply(Reply::Plan(Ok(plan("v2", "s2", 2.0))));
    let c = app.plan.conflict.clone().expect("conflict");
    assert_eq!(c.remote_content.as_deref(), Some("v2"));
    assert_eq!(app.plan.buffer, "v1 local");

    // Keep mine: adopt remote sha as expectation, stay dirty.
    app.plan.resolve_keep_mine();
    assert!(app.plan.conflict.is_none());
    assert_eq!(app.plan.sha256.as_deref(), Some("s2"));
    let req = app.plan.save_request(Some("C:/p".into()), None);
    assert_eq!(req.expected_sha256.as_deref(), Some("s2"));
    assert_eq!(req.content, "v1 local");

    // Pre-flight drift (expectedSha256/expectedMtimeMs) blocks the save.
    app.handle_reply(Reply::SavePlan(Ok(SavePlanOutcome::Drift(plan("v3", "s3", 3.0)))));
    assert!(app.plan.conflict.as_ref().unwrap().reason.contains("expected sha256"));
    assert!(app.plan.resolve_take_theirs());
    assert_eq!(app.plan.buffer, "v3");
    assert_eq!(app.plan.mtime_ms, Some(3.0));

    // Server-side 409 conflict without content -> conflict requiring reload.
    app.plan.buffer.push('!');
    app.handle_reply(Reply::SavePlan(Ok(SavePlanOutcome::Saved(SavePlanResponse {
        success: false,
        conflict: Some(true),
        sha256: Some("s4".into()),
        mtime_ms: Some(4.0),
        tasks_updated: None,
        error: Some("sha mismatch".into()),
    }))));
    assert!(app.plan.conflict.as_ref().unwrap().remote_content.is_none());
    assert!(!app.plan.resolve_take_theirs());

    // Successful save clears dirty + updates hash.
    app.plan.conflict = None;
    app.handle_reply(Reply::SavePlan(Ok(SavePlanOutcome::Saved(SavePlanResponse {
        success: true,
        conflict: None,
        sha256: Some("s5".into()),
        mtime_ms: Some(5.0),
        tasks_updated: Some(2),
        error: None,
    }))));
    assert!(!app.plan.is_dirty());
    assert_eq!(app.plan.sha256.as_deref(), Some("s5"));
}

#[test]
fn copy_resume_command_uses_clipboard() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);
    let mut copied = false;
    let out = ctx.run(egui::RawInput::default(), |c| copied = app.copy_resume(c));
    assert!(copied);
    assert_eq!(out.platform_output.copied_text, "claude --resume cc-111");
}

#[test]
fn session_selection_and_project_switch_reset_state() {
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);
    app.select_session(0);
    assert_eq!(app.chat.harness(), "claude-code");
    assert_eq!(app.active_session_id.as_deref(), Some("asd-1"));
    assert_eq!(app.chat.picker.selected.as_deref(), Some("sonnet"));

    // Switching to an unpromoted CLI session with model=None must clear stale model & keep active_session_id=None
    app.select_session(1);
    assert_eq!(app.chat.harness(), "codex");
    assert!(app.active_session_id.is_none(), "unpromoted CLI session has no ASD id");
    assert!(app.selected_session.is_some());
    assert!(app.chat.picker.selected.is_none(), "switching to default-model session must clear previous model");

    // Reply::Sessions arriving while an unpromoted CLI session is selected must NOT hijack active_session_id
    let sessions_copy = app.sessions.clone();
    app.handle_reply(Reply::Sessions(Ok(GetSessionsResponse {
        sessions: sessions_copy,
        active_session_id: Some("asd-1".into()),
    })));
    assert!(
        app.active_session_id.is_none(),
        "Reply::Sessions must not overwrite active_session_id when an unpromoted CLI session is selected"
    );

    // Sending a prompt on the unpromoted CLI session passes its harness_session_id and explicit "default" reset
    app.chat.turn_seq = None;
    app.backend.set_client(Some(NexusClient::new("http://127.0.0.1:9", None)));
    app.backend.submitted.clear();
    app.chat.composer = "resume unpromoted cli".into();
    app.send_prompt();
    let sent_unpromoted = app.backend.submitted.iter().any(|r| match r {
        Request::StartTurn { req, .. } => {
            req.session_id.as_deref() == Some("cx-222")
                && req.harness == "codex"
                && req.model.as_deref() == Some("default")
        }
        _ => false,
    });
    assert!(sent_unpromoted, "send_prompt on unpromoted CLI session must pass harness_session_id and model='default'");
    app.backend.set_client(None);

    app.chat.turn_seq = None;
    app.switch_project("C:/other".into());
    assert_eq!(app.project_root, "C:/other");
    assert!(app.sessions.is_empty());
    assert!(app.board.board.is_none());
    assert!(!app.plan.loaded);
    assert!(!app.routing.loaded);
    assert!(app.projects.iter().any(|p| p.project_root == "C:/other"));
    // offline: requests degrade to a status message, no panic
    assert!(app.status.contains("not connected"));
}

#[test]
fn connection_failure_is_reported_and_ui_still_renders() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.backend.inject(Reply::Connected(Err("node not found".into())));
    let _ = frame(&ctx, &mut app, vec![]);
    assert!(matches!(app.conn, ConnState::Failed(_)));
    assert!(app.status.contains("node not found"));
    app.composer_send_offline_check();
}

#[test]
fn composer_enter_sends_and_shift_enter_inserts_newline() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.backend.inject(Reply::Connected(Err("offline".into())));
    app.chat.composer = "hello".into();

    ctx.memory_mut(|m| m.request_focus(egui::Id::new("chat-composer")));
    let _ = frame(&ctx, &mut app, vec![]);

    let key = |modifiers| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };

    let _ = frame(&ctx, &mut app, vec![key(egui::Modifiers::SHIFT)]);
    assert_eq!(app.chat.composer, "hello\n", "Shift+Enter must insert a newline");
    assert!(!app.chat.is_streaming());

    app.status.clear();
    let _ = frame(&ctx, &mut app, vec![key(egui::Modifiers::NONE)]);
    assert!(!app.chat.is_streaming());
    assert!(
        app.status.contains("not connected"),
        "plain Enter must attempt a send; status={:?}",
        app.status
    );
    assert_eq!(app.chat.composer, "hello\n", "prompt kept when send fails");
}

#[test]
fn thinking_card_toggles_when_clicked_anywhere() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    app.chat.timeline.begin_turn("explain");
    app.chat.timeline.push(
        EntryKind::Thinking,
        "",
        "line 1 of thought\nline 2 of thought\nline 3 of thought\nline 4 of thought",
    );
    let think_id = app.chat.timeline.entries[1].id;

    let _ = frame(&ctx, &mut app, vec![]);
    assert!(!is_thinking_open(&ctx, think_id));
    let rect = thinking_card_rect(&ctx, think_id).expect("thinking card rect");

    let click_at = |ctx: &egui::Context, app: &mut NexusApp, pos: egui::Pos2| {
        let _ = frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = frame(
            ctx,
            app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        // Settle repaint frame so expanded/collapsed layout rect updates
        let _ = frame(ctx, app, vec![]);
    };

    // 1. Click far-right empty area of Thinking header -> expands
    click_at(&ctx, &mut app, egui::pos2(rect.max.x - 12.0, rect.center().y));
    assert!(is_thinking_open(&ctx, think_id), "clicking right margin of Thinking must expand");

    // 2. Click inside expanded Thinking body -> collapses
    let open_rect = thinking_card_rect(&ctx, think_id).unwrap();
    assert!(open_rect.height() > rect.height(), "expanded Thinking card must grow taller");
    click_at(&ctx, &mut app, egui::pos2(open_rect.center().x, open_rect.max.y - 8.0));
    assert!(!is_thinking_open(&ctx, think_id), "clicking expanded body of Thinking must collapse");

    // 3. Click center text of collapsed Thinking header -> expands again
    let closed_rect = thinking_card_rect(&ctx, think_id).unwrap();
    click_at(&ctx, &mut app, closed_rect.center());
    assert!(is_thinking_open(&ctx, think_id), "clicking center label of Thinking must expand");
}

#[test]
fn switching_sessions_or_new_chat_preserves_running_stream() {
    let ctx = ctx();
    let mut app = NexusApp::new("C:/p", RunOptions::default());
    populate(&mut app);
    app.chat.turn_seq = None;
    app.chat.timeline.clear();
    app.backend.set_client(Some(NexusClient::new("http://127.0.0.1:9", None)));

    // 1. Select Session 0 (asd-1 / cc-111) and start a streaming turn
    app.select_session(0);
    app.backend.submitted.clear();
    app.chat.composer = "지금은 어느 모델이야?".into();
    app.send_prompt();
    let seq_a = app.chat.turn_seq.expect("Session 0 turn must be streaming");
    assert!(app.chat.is_streaming());

    // 2. While Session 0 is streaming, switch to Session 1 (cx-222) to check it
    app.select_session(1);
    assert!(
        !app.backend.submitted.iter().any(|r| matches!(r, Request::CancelTurn(_))),
        "switching sessions must NEVER cancel the running turn"
    );
    assert!(!app.chat.is_streaming(), "Session 1 itself is not streaming");
    assert!(app.has_running_background_turns(), "Session 0 keeps streaming in background");

    // Session 1's transcript loads cleanly without touching Session 0
    app.handle_reply(Reply::Transcript(Ok(GetTranscriptResponse {
        harness: "codex".into(),
        harness_session_id: "cx-222".into(),
        transcript_path: None,
        messages: vec![NormalizedTranscriptMessage {
            role: "assistant".into(),
            category: Some(TranscriptCategory::Assistant),
            text: "codex history message".into(),
            thinking: None,
            tool_name: None,
            tool_use: None,
            tool_result: None,
            timestamp: None,
        }],
        has_more: None,
        truncated: None,
        error: None,
    })));
    assert_eq!(app.chat.timeline.entries.len(), 1);
    assert_eq!(app.chat.timeline.entries[0].body, "codex history message");

    // Meanwhile Session 0 receives SSE chunks in the background
    app.handle_reply(Reply::Stream {
        seq: seq_a,
        msg: ev(r#"{"kind":"thinking","thinking":"checking model"}"#),
    });
    app.handle_reply(Reply::Stream {
        seq: seq_a,
        msg: ev(r#"{"kind":"text","text":"현재 Gemini 4 "}"#),
    });

    // Render sidebar with RUNNING badge in both DarkColor & Monochrome
    let _ = frame(&ctx, &mut app, vec![]);
    app.set_theme_mode(&ctx, ThemeMode::Monochrome);
    let mono_out = frame(&ctx, &mut app, vec![]);
    assert!(assert_grayscale_output(&ctx, mono_out) > 100);
    app.set_theme_mode(&ctx, ThemeMode::DarkColor);

    // 3. Switch back to Session 0 while it is STILL streaming -> live stream reattaches!
    app.select_session(0);
    assert!(app.chat.is_streaming(), "reselecting Session 0 must reattach live stream");
    assert_eq!(app.chat.turn_seq, Some(seq_a));
    assert!(app.chat.timeline.entries.iter().any(|e| e.body == "지금은 어느 모델이야?"));
    assert!(app.chat.timeline.entries.iter().any(|e| e.body == "현재 Gemini 4 "));

    // 4. Click `+ New Chat` while Session 0 is still streaming -> parks instead of cancelling!
    app.backend.submitted.clear();
    app.start_new_session_draft();
    assert!(
        !app.backend.submitted.iter().any(|r| matches!(r, Request::CancelTurn(_))),
        "+ New Chat must NEVER cancel a running turn in another session"
    );
    assert!(!app.chat.is_streaming());
    assert!(app.has_running_background_turns());

    // Session 0 finishes its answer while user is on `+ New Chat`
    app.handle_reply(Reply::Stream {
        seq: seq_a,
        msg: ev(r#"{"kind":"text","text":"Neon 모델입니다."}"#),
    });
    app.handle_reply(Reply::Stream { seq: seq_a, msg: StreamMessage::Done });
    assert!(!app.has_running_background_turns());

    // 5. Reselect Session 0 -> full completed answer is immediately present!
    app.select_session(0);
    assert!(!app.chat.is_streaming());
    assert!(
        app.chat
            .timeline
            .entries
            .iter()
            .any(|e| e.kind == EntryKind::Text && e.body == "현재 Gemini 4 Neon 모델입니다."),
        "completed background answer must be preserved when returning to Session 0"
    );
}

trait OfflineCheck {
    fn composer_send_offline_check(&mut self);
}

impl OfflineCheck for NexusApp {
    fn composer_send_offline_check(&mut self) {
        self.chat.composer = "hi".into();
        self.send_prompt();
        assert!(!self.chat.is_streaming());
        assert_eq!(self.chat.composer, "hi", "prompt kept when send fails");
    }
}
