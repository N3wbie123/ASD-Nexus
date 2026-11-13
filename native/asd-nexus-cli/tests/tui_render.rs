use std::sync::mpsc;

use asd_nexus_cli::app::{Action, App, BoardPane, Focus, Overlay, PromptPurpose};
use asd_nexus_cli::theme;
use asd_nexus_cli::timeline::EntryKind;
use asd_nexus_cli::ui;
use asd_nexus_client::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}
fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}
fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn render(app: &App) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    t.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
    let w = buf.area.width as usize;
    buf.content()
        .chunks(w)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_monochrome(buf: &Buffer) {
    for (i, c) in buf.content().iter().enumerate() {
        assert!(theme::is_grayscale(c.fg), "cell {i} fg {:?} ({:?})", c.fg, c.symbol());
        assert!(theme::is_grayscale(c.bg), "cell {i} bg {:?} ({:?})", c.bg, c.symbol());
    }
}

fn session(h: &str, id: &str, asd: Option<&str>, phase: Option<&str>) -> DiscoveredSessionItem {
    DiscoveredSessionItem {
        harness: h.into(),
        harness_session_id: id.into(),
        project_root: "C:/repo".into(),
        updated_at: 0.0,
        model: None,
        summary: Some(format!("summary {id}")),
        transcript_path: None,
        asd_session_id: asd.map(Into::into),
        asd_phase: phase.map(Into::into),
        is_promoted_to_asd: asd.is_some(),
    }
}

fn app() -> App {
    let mut a = App::new("C:/repo", "claude-code", None);
    a.take_actions();
    a.projects = vec![ProjectSummaryItem {
        project_root: "C:/repo".into(),
        project_hash: "h".into(),
        session_count: Some(3),
        last_active_at: None,
    }];
    a.set_sessions(GetSessionsResponse {
        sessions: vec![
            session("claude-code", "cc-1", Some("asd-1"), Some("planning")),
            session("codex", "cx-1", None, None),
            session("cursor", "cu-1", Some("asd-3"), Some("done")),
        ],
        active_session_id: Some("asd-1".into()),
    });
    a.board = Some(GetBoardResponse {
        session_id: Some("asd-1".into()),
        phase: Some("running".into()),
        progress: Some("1/2".into()),
        tasks: vec![BoardTaskItem {
            id: "T1".into(),
            subject: "Build TUI".into(),
            status: "review".into(),
            assignee_role: Some("worker".into()),
            difficulty: None,
            turn: Some(1),
            review_verdict: None,
            summary: None,
        }],
    });
    a
}

#[test]
fn base_screen_renders_monochrome() {
    let a = app();
    let buf = render(&a);
    let s = text(&buf);
    assert!(s.contains("ASD-NEXUS"));
    assert!(s.contains("PROJECTS"));
    assert!(s.contains("SESSIONS"));
    assert!(s.contains("TIMELINE"));
    assert!(s.contains("TOKENS"));
    assert_monochrome(&buf);
}

#[test]
fn ctrl_overlays_toggle_load_and_render() {
    let cases: [(char, Overlay, &str, Action); 5] = [
        ('b', Overlay::Board, "PROGRESS BOARD", Action::RefreshBoard),
        ('l', Overlay::Subagents, "SUBAGENTS", Action::RefreshSubagents),
        ('d', Overlay::Diff, "WORKTREE DIFF", Action::RefreshBoard),
        ('r', Overlay::Routing, "ROUTING CONFIG", Action::LoadRouting),
        ('t', Overlay::Terminal, "INTERACTIVE RESUME", Action::LoadResume("asd-1".into())),
    ];
    for (c, ov, title, first) in cases {
        let mut a = app();
        a.handle_key(ctrl(c));
        assert_eq!(a.overlay, Some(ov), "ctrl+{c}");
        assert_eq!(a.take_actions().first(), Some(&first), "ctrl+{c}");
        let buf = render(&a);
        assert!(text(&buf).contains(title), "ctrl+{c} title");
        assert_monochrome(&buf);
        a.handle_key(ctrl(c));
        assert_eq!(a.overlay, None, "ctrl+{c} toggles off");
        a.handle_key(ctrl(c));
        a.handle_key(key(KeyCode::Esc));
        assert_eq!(a.overlay, None, "esc closes ctrl+{c}");
    }
}

#[test]
fn board_overlay_actions() {
    let mut a = app();
    a.handle_key(ctrl('b'));
    a.take_actions();
    a.handle_key(ch('s'));
    a.handle_key(ch('g'));
    assert_eq!(a.take_actions(), vec![Action::SyncPlan, Action::Dispatch { mode: "parallel_spawn".into() }]);
    a.handle_key(key(KeyCode::Tab));
    assert_eq!(a.board_pane, BoardPane::Plan);
    a.handle_key(ch('b'));
    for c in "scope".chars() {
        a.handle_key(ch(c));
    }
    a.handle_key(key(KeyCode::Enter));
    assert!(matches!(a.prompt.as_ref().unwrap().purpose, PromptPurpose::DebateProposal { .. }));
    for c in "use ratatui".chars() {
        a.handle_key(ch(c));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::Debate { topic: "scope".into(), proposal: "use ratatui".into() }]);
}

#[test]
fn subagent_overlay_actions_and_log_tail() {
    let mut a = app();
    a.subagents = vec![SubagentRecordItem {
        sub_session_id: "sub-1".into(),
        parent_session_id: "asd-1".into(),
        task_id: Some("T1".into()),
        role: Some("worker".into()),
        harness: Some("codex".into()),
        model: None,
        status: "running".into(),
        start_time: None,
        end_time: None,
        exit_code: None,
        log_path: None,
        prompt_path: None,
    }];
    a.logs = vec!["line one".into(), "line two".into()];
    a.handle_key(ctrl('l'));
    a.take_actions();
    let s = text(&render(&a));
    assert!(s.contains("sub-1") && s.contains("line two"));
    for (c, kind) in [
        ('r', SubagentControlActionKind::Retry),
        ('f', SubagentControlActionKind::Fallback),
        ('u', SubagentControlActionKind::Resume),
    ] {
        a.handle_key(ch(c));
        assert_eq!(
            a.take_actions(),
            vec![Action::SubagentAction { sub_session_id: "sub-1".into(), action: kind, reason: None }]
        );
    }
    a.handle_key(ch('x'));
    assert!(a.confirm.is_some());
    a.handle_key(ch('y'));
    assert_eq!(
        a.take_actions(),
        vec![Action::SubagentAction {
            sub_session_id: "sub-1".into(),
            action: SubagentControlActionKind::Kill,
            reason: None
        }]
    );
    a.handle_key(ch('c'));
    for c in "go".chars() {
        a.handle_key(ch(c));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(
        a.take_actions(),
        vec![Action::SubagentAction {
            sub_session_id: "sub-1".into(),
            action: SubagentControlActionKind::Custom,
            reason: Some("go".into())
        }]
    );
}

#[test]
fn diff_overlay_approve_reject() {
    let mut a = app();
    a.handle_key(ctrl('d'));
    a.take_actions();
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::LoadDiff("T1".into())]);
    a.diff = Some(WorktreeDiffResponse {
        task_id: Some("T1".into()),
        worktree_path: Some(".asd/wt/1-T1".into()),
        mode: Some("working".into()),
        base_ref: Some("HEAD".into()),
        files: vec![WorktreeDiffFile { path: "src/a.rs".into(), status: Some("M".into()), additions: Some(2), deletions: Some(1) }],
        status_short: None,
        diff_text: Some("diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new".into()),
        error: None,
    });
    let buf = render(&a);
    assert!(text(&buf).contains("src/a.rs"));
    assert_monochrome(&buf);
    a.handle_key(ch('a'));
    a.handle_key(ch('y'));
    assert_eq!(a.take_actions(), vec![Action::Approve("T1".into())]);
    a.handle_key(ch('x'));
    a.handle_key(ch('n'));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::Reject { task_id: "T1".into(), reason: Some("n".into()) }]);
}

#[test]
fn routing_editor_saves_valid_json_only() {
    let mut a = app();
    a.routing = Some(serde_json::json!({"main": "claude-code"}));
    a.handle_key(ctrl('r'));
    a.take_actions();
    assert!(text(&render(&a)).contains("\"main\""));
    a.handle_key(ch('e'));
    assert!(a.routing_editor.is_some());
    // Break JSON: insert a stray char at start.
    a.handle_key(ch('x'));
    a.handle_key(ctrl('s'));
    assert!(a.take_actions().is_empty());
    assert!(a.status.contains("Invalid JSON"));
    a.handle_key(key(KeyCode::Backspace));
    a.handle_key(ctrl('s'));
    match a.take_actions().as_slice() {
        [Action::SaveRouting(s)] => assert!(s.contains("claude-code")),
        other => panic!("{other:?}"),
    }
    a.handle_key(ch('x'));
    a.handle_key(ch('y'));
    assert_eq!(a.take_actions(), vec![Action::ResetRouting]);
}

#[test]
fn terminal_overlay_launch() {
    let mut a = app();
    a.handle_key(ctrl('t'));
    a.take_actions();
    a.resume = Some(GetResumeCommandResponse {
        success: true,
        harness: "claude-code".into(),
        harness_session_id: Some("cc-1".into()),
        command: "claude".into(),
        args: vec!["--resume".into(), "cc-1".into()],
        cli_string: "claude --resume cc-1".into(),
        cwd: "C:/repo".into(),
        error: None,
    });
    let buf = render(&a);
    assert!(text(&buf).contains("claude --resume cc-1"));
    assert_monochrome(&buf);
    a.handle_key(ch('k'));
    assert_eq!(a.terminal_kind, 2);
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::LaunchTerminal { session_id: "asd-1".into() }]);
}

#[test]
fn session_explorer_keys() {
    let mut a = app();
    assert_eq!(a.focus, Focus::Sessions);
    a.handle_key(key(KeyCode::Enter));
    assert!(matches!(a.take_actions().as_slice(), [Action::OpenSession(s)] if s.harness_session_id == "cc-1"));
    a.handle_key(ch('f'));
    a.handle_key(ch('t'));
    a.handle_key(ch('d'));
    assert_eq!(
        a.take_actions(),
        vec![
            Action::SetForeground("asd-1".into()),
            Action::SetPhase { session_id: "asd-1".into(), phase: SessionPhase::Running },
            Action::Demote("asd-1".into()),
        ]
    );
    a.handle_key(ch('x'));
    a.handle_key(ch('y'));
    assert_eq!(a.take_actions(), vec![Action::Delete("asd-1".into())]);
    a.handle_key(ch('u'));
    a.handle_key(ch('n'));
    assert!(a.take_actions().is_empty());
    a.handle_key(ch('e'));
    a.handle_key(ch('!'));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SaveMemo { session_id: "asd-1".into(), memo: "summary cc-1!".into() }]);
    // Promote the unpromoted codex session.
    a.handle_key(key(KeyCode::Down));
    a.handle_key(ch('p'));
    assert!(matches!(a.take_actions().as_slice(), [Action::Promote(s)] if s.harness_session_id == "cx-1"));
    a.handle_key(ch('n'));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::CreateSession { title: None }]);
}

#[test]
fn session_search_and_harness_filter() {
    let mut a = app();
    a.handle_key(ch('/'));
    for c in "cu-".chars() {
        a.handle_key(ch(c));
    }
    assert_eq!(a.filtered_sessions(), vec![2]);
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.search, "cu-");
    a.handle_key(ch('/'));
    a.handle_key(key(KeyCode::Esc));
    assert_eq!(a.filtered_sessions().len(), 3);
    a.handle_key(ch('h')); // claude-code
    a.handle_key(ch('h')); // codex
    assert_eq!(a.filtered_sessions(), vec![1]);
    assert!(text(&render(&a)).contains("SESSIONS [codex]"));
}

#[test]
fn model_picker_search_and_select() {
    let mut a = app();
    a.models.insert("codex".into(), vec!["gpt-5".into(), "gpt-5-mini".into(), "o3".into()]);
    a.handle_key(ctrl('k'));
    assert_eq!(a.overlay, Some(Overlay::ModelPicker));
    a.handle_key(key(KeyCode::Tab)); // -> codex
    for c in "mini".chars() {
        a.handle_key(ch(c));
    }
    assert_eq!(a.picker_models(), vec!["(default)".to_string(), "gpt-5-mini".into()]);
    let buf = render(&a);
    assert!(text(&buf).contains("gpt-5-mini"));
    assert_monochrome(&buf);
    a.handle_key(key(KeyCode::F(5)));
    assert_eq!(a.take_actions(), vec![Action::RefreshModels { force: true }]);
    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.harness, "codex");
    assert_eq!(a.model.as_deref(), Some("gpt-5-mini"));
    assert_eq!(a.overlay, None);
}

#[test]
fn project_switcher() {
    let mut a = app();
    a.handle_key(key(KeyCode::BackTab));
    assert_eq!(a.focus, Focus::Projects);
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SelectProject("C:/repo".into())]);
}

#[test]
fn streaming_timeline_over_mpsc_and_cancel() {
    let mut a = app();
    a.focus = Focus::Input;
    for c in "hello".chars() {
        a.handle_key(ch(c));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SendPrompt("hello".into())]);

    // Simulate exec: begin turn + attach an mpsc receiver fed by an SSE parser.
    a.timeline.begin_turn("hello");
    let (tx, rx) = mpsc::channel();
    a.stream = Some(asd_nexus_cli::app::ActiveStream { handle: None, rx });
    let mut p = SseParser::new();
    let raw = concat!(
        "data: {\"kind\":\"thinking\",\"thinking\":\"pondering\"}\n\n",
        "data: {\"kind\":\"tool_use\",\"toolUse\":{\"name\":\"grep\",\"id\":\"1\",\"input\":\"foo\"}}\n\n",
    );
    for m in p.feed(raw.as_bytes()) {
        tx.send(m).unwrap();
    }
    a.drain_stream();
    assert!(a.is_streaming());
    let s = text(&render(&a));
    assert!(s.contains("THINKING [+]") && s.contains("TOOL [+] grep") && s.contains("STREAMING"));

    a.handle_key(ctrl('x'));
    assert_eq!(a.take_actions(), vec![Action::CancelTurn]);

    let raw2 = concat!(
        "data: {\"kind\":\"tool_result\",\"toolResult\":{\"toolUseId\":\"1\",\"output\":\"hit\"}}\n\n",
        "data: {\"kind\":\"text\",\"text\":\"Answer\"}\n\n",
        "data: {\"kind\":\"usage\",\"usage\":{\"inputTokens\":1200,\"outputTokens\":34,\"costUsd\":0.0123}}\n\n",
        "data: {\"kind\":\"status\",\"status\":\"done\"}\n\n",
    );
    for m in p.feed(raw2.as_bytes()) {
        tx.send(m).unwrap();
    }
    drop(tx);
    a.drain_stream();
    assert!(!a.is_streaming());
    let kinds: Vec<_> = a.timeline.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![EntryKind::User, EntryKind::Thinking, EntryKind::ToolUse, EntryKind::ToolResult, EntryKind::Text, EntryKind::System]
    );
    assert_eq!(a.timeline.entries[3].title, "grep");
    let buf = render(&a);
    let s = text(&buf);
    assert!(s.contains("Answer"));
    assert!(s.contains("1,200") && s.contains("$0.0123"));
    assert_monochrome(&buf);
}

#[test]
fn timeline_collapse_via_keys_and_error_badge() {
    let mut a = app();
    a.timeline.apply(&StreamMessage::Event(Box::new(
        serde_json::from_str(r#"{"kind":"thinking","thinking":"secret line one\nline two"}"#).unwrap(),
    )));
    a.timeline.apply(&StreamMessage::Event(Box::new(
        serde_json::from_str(r#"{"kind":"tool_result","toolResult":{"name":"bash","output":"denied","isError":true}}"#).unwrap(),
    )));
    a.timeline.apply(&StreamMessage::Error("network down".into()));
    a.focus = Focus::Timeline;
    a.timeline_sel = 0;
    let s = text(&render(&a));
    assert!(!s.contains("line two"));
    assert!(s.contains("ERROR"));
    a.handle_key(key(KeyCode::Enter));
    assert!(!a.timeline.entries[0].collapsed);
    let buf = render(&a);
    assert!(text(&buf).contains("line two"));
    assert_monochrome(&buf);
    a.handle_key(ctrl('e'));
    assert!(a.timeline.entries.iter().filter(|e| e.kind.collapsible()).all(|e| e.collapsed));
}

#[test]
fn help_and_quit() {
    let mut a = app();
    a.handle_key(key(KeyCode::F(1)));
    assert_eq!(a.overlay, Some(Overlay::Help));
    assert_monochrome(&render(&a));
    a.handle_key(key(KeyCode::Esc));
    a.handle_key(ctrl('q'));
    assert!(a.should_quit);
}
