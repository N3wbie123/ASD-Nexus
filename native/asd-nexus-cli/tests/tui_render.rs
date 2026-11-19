use std::sync::mpsc;

use asd_nexus_cli::app::{Action, App, BoardPane, Focus, Overlay, PromptPurpose};
use asd_nexus_cli::theme::{self, ThemeMode};
use asd_nexus_cli::timeline::{Entry, EntryKind};
use asd_nexus_cli::ui;
use asd_nexus_client::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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
fn shift_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
}
fn alt_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
}
fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(ch(c));
    }
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

fn assert_has_color(buf: &Buffer) {
    let colored = buf
        .content()
        .iter()
        .any(|c| !theme::is_grayscale(c.fg) || !theme::is_grayscale(c.bg));
    assert!(colored, "expected DarkColor buffer to contain chromatic cells");
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
        auto_approve: None,
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
    a.harness_commands = vec![
        HarnessCommandItem {
            name: "compact".into(),
            description: "Compact conversation history".into(),
            harness: "claude-code".into(),
            source: HarnessCommandSource::Builtin,
            kind: HarnessCommandKind::Command,
            path: None,
        },
        HarnessCommandItem {
            name: "asd-plan".into(),
            description: "Plan-only ASD workflow".into(),
            harness: "claude-code".into(),
            source: HarnessCommandSource::Global,
            kind: HarnessCommandKind::Skill,
            path: Some("~/.claude/skills/asd-plan/SKILL.md".into()),
        },
    ];
    a
}

#[test]
fn home_vs_active_chat_screen_and_optional_sidebar() {
    let mut a = app();
    assert!(a.is_home());
    assert!(!a.sidebar_open);
    assert_eq!(a.focus, Focus::Input);

    // Home screen: pure black background, single unified project path, no OpenCode UX or command hints
    let home_buf = render(&a);
    let home_txt = text(&home_buf);
    assert!(home_txt.contains("ASD-NEXUS"));
    assert_eq!(home_txt.matches("C:/repo").count(), 1, "project_root must appear in one unified place only");
    assert!(!home_txt.contains("OpenCode UX"));
    assert!(!home_txt.contains("Ctrl+P palette"));
    assert!(!home_txt.contains("/sessions"));
    assert!(!home_txt.contains("PROJECTS"));

    // Verify pure black background on main canvas in DarkColor mode
    let bg_cell = &home_buf[(0, 1)]; // gap row directly below header
    assert_eq!(bg_cell.bg, ratatui::style::Color::Rgb(0, 0, 0));

    // Verify input card symmetry & zero vertical shift when typing
    let empty_rect = a.ui_regions.borrow().input_rect.unwrap();
    for dy in 0..empty_rect.height {
        let sym = home_buf[(empty_rect.x, empty_rect.y + dy)].symbol();
        assert_eq!(sym, "┃", "every row of input card must carry left bar (row {dy})");
    }
    type_str(&mut a, "hi");
    let typed_buf = render(&a);
    let typed_rect = a.ui_regions.borrow().input_rect.unwrap();
    assert_eq!(empty_rect, typed_rect, "typing 1 line must not shift input card");
    for dy in 0..typed_rect.height {
        let sym = typed_buf[(typed_rect.x, typed_rect.y + dy)].symbol();
        assert_eq!(sym, "┃", "every row of typed input card must carry left bar (row {dy})");
    }
    a.handle_key(key(KeyCode::Backspace));
    a.handle_key(key(KeyCode::Backspace));

    // Transition to Active chat screen when timeline has entries
    a.timeline.push(Entry::new(EntryKind::User, "Hello world"));
    a.timeline.push(Entry::new(EntryKind::Thinking, "analyzing request"));
    a.timeline.push(Entry::new(EntryKind::Text, "Clean white response"));
    assert!(!a.is_home());
    let chat_buf = render(&a);
    let chat_txt = text(&chat_buf);
    // Header separation: row 1 is pure empty gap so ASD-NEXUS never connects to user bar
    let row1: String = (0..chat_buf.area.width).map(|x| chat_buf[(x, 1)].symbol()).collect();
    assert!(row1.trim().is_empty(), "row below header must be an empty spacer");
    // User message has left bar only, no YOU label
    assert!(chat_txt.contains("┃ Hello world"));
    assert!(!chat_txt.contains("YOU"));
    // AI Thinking & Response have NO left bar and NO ASSISTANT label
    assert!(chat_txt.contains("▸ Thinking · analyzing request"));
    assert!(!chat_txt.contains("┃ ▸ Thinking"));
    assert!(chat_txt.contains("Clean white response"));
    assert!(!chat_txt.contains("ASSISTANT"));
    assert!(!chat_txt.contains("┃ Clean white response"));
    assert_eq!(chat_txt.matches("C:/repo").count(), 1);
    assert!(!chat_txt.contains("PROJECTS"));

    // Verify AI Thinking is gray and AI Response is pure white in DarkColor mode
    let lines: Vec<&str> = chat_txt.lines().collect();
    let think_y = lines.iter().position(|l| l.contains("▸ Thinking")).unwrap() as u16;
    let think_x = lines[think_y as usize].chars().position(|c| c == '▸').unwrap() as u16;
    assert!(theme::is_grayscale(chat_buf[(think_x, think_y)].fg), "AI thinking must be gray");
    let ans_y = lines.iter().position(|l| l.contains("Clean white response")).unwrap() as u16;
    let ans_x = lines[ans_y as usize].chars().position(|c| c == 'C').unwrap() as u16;
    assert_eq!(chat_buf[(ans_x, ans_y)].fg, ratatui::style::Color::Rgb(255, 255, 255));

    // Optional `/sidebar` toggles left drawer
    type_str(&mut a, "/sidebar");
    a.handle_key(key(KeyCode::Enter));
    assert!(a.sidebar_open);
    let sb_txt = text(&render(&a));
    assert!(sb_txt.contains("PROJECTS"));
    assert!(sb_txt.contains("SESSIONS"));
}

#[test]
fn dark_color_default_and_strict_monochrome_theme_toggle() {
    let mut a = app();
    assert_eq!(a.theme_mode, ThemeMode::DarkColor);
    let dark_buf = render(&a);
    assert_has_color(&dark_buf);

    // Toggle via `/theme` slash command -> Monochrome
    type_str(&mut a, "/theme");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.theme_mode, ThemeMode::Monochrome);
    assert_monochrome(&render(&a));

    // Also verify Active chat screen + overlays in Monochrome pass strict grayscale check
    a.timeline.push(Entry::new(EntryKind::User, "prompt"));
    a.timeline.push(Entry::new(EntryKind::Thinking, "thinking details"));
    a.timeline.push(Entry::shell("git status", "On branch main", 0));
    assert_monochrome(&render(&a));

    // Toggle back to DarkColor
    type_str(&mut a, "/theme");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.theme_mode, ThemeMode::DarkColor);
    assert_has_color(&render(&a));
}

#[test]
fn slash_popup_nexus_and_harness_commands_and_ctrl_p_palette() {
    let mut a = app();
    // Typing `/` opens the real-time slash popup above input
    a.handle_key(ch('/'));
    assert!(a.is_slash_popup_active());
    let popup_buf = render(&a);
    let popup_txt = text(&popup_buf);
    assert!(popup_txt.contains("SLASH COMMANDS"));
    assert!(popup_txt.contains("[nexus]"));

    // Filter for harness-specific command `/compact`
    type_str(&mut a, "comp");
    assert!(a.is_slash_popup_active());
    let cmds = a.filtered_slash_commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].name, "compact");
    assert!(!cmds[0].is_builtin);
    let filt_txt = text(&render(&a));
    assert!(filt_txt.contains("/compact"));
    assert!(filt_txt.contains("[claude-code]"));

    // Pressing Enter on a harness-specific `/command` sends `/compact` to turn stream
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SendPrompt("/compact".into())]);
    assert!(a.input.is_empty());

    // Ctrl+P opens Command Palette with both Nexus and harness commands
    a.handle_key(ctrl('p'));
    assert_eq!(a.overlay, Some(Overlay::CommandPalette));
    let pal_txt = text(&render(&a));
    assert!(pal_txt.contains("COMMAND PALETTE"));
    assert!(pal_txt.contains("[nexus]"));

    type_str(&mut a, "asd-plan");
    let items = a.palette_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "asd-plan");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, None);
    assert_eq!(a.take_actions(), vec![Action::SendPrompt("/asd-plan".into())]);
}

#[test]
fn cd_slash_command_direct_and_modal() {
    let mut a = app();
    // `/cd <path>` switches project directly
    type_str(&mut a, "/cd D:/other-project");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SelectProject("D:/other-project".into())]);

    // `/cd` without args opens DirPicker modal and requests ListDirs
    type_str(&mut a, "/cd");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, Some(Overlay::DirPicker));
    assert_eq!(a.take_actions(), vec![Action::ListDirs { prefix: None }]);

    a.dir_candidates = vec![FsListEntry {
        path: "packages/web".into(),
        name: "web".into(),
        is_dir: true,
    }];
    let modal_txt = text(&render(&a));
    assert!(modal_txt.contains("SWITCH DIRECTORY"));
    assert!(modal_txt.contains("packages/web"));

    // Select the second item (`packages/web`) and press Enter
    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, None);
    assert_eq!(a.take_actions(), vec![Action::SelectProject("C:/repo/packages/web".into())]);
}

#[test]
fn at_file_autocomplete_and_bang_shell_execution() {
    let mut a = app();
    a.file_candidates = vec![
        FsListEntry { path: "src/main.rs".into(), name: "main.rs".into(), is_dir: false },
        FsListEntry { path: "src/app.rs".into(), name: "app.rs".into(), is_dir: false },
    ];

    // Typing `@ap` triggers `ListFiles` and opens the `@FILE AUTOCOMPLETE` popup
    type_str(&mut a, "review @ap");
    assert!(a.is_file_popup_active());
    let actions = a.take_actions();
    assert!(actions.contains(&Action::ListFiles { prefix: Some("ap".into()) }));
    let pop_txt = text(&render(&a));
    assert!(pop_txt.contains("@FILE AUTOCOMPLETE"));
    assert!(pop_txt.contains("@src/app.rs"));

    // Tab inserts `@src/app.rs ` into input at the token position
    a.handle_key(key(KeyCode::Tab));
    assert!(!a.is_file_popup_active());
    assert_eq!(a.input, "review @src/app.rs ");

    // Clear input and run `!echo hello_nexus` shell command
    type_str(&mut a, "");
    a.input.clear();
    a.input_area.clear();
    type_str(&mut a, "!echo hello_nexus");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::RunShell("echo hello_nexus".into())]);

    // Execute local shell and verify timeline records Shell output + exit code
    a.execute_shell("echo hello_nexus");
    assert_eq!(a.timeline.entries.len(), 1);
    assert_eq!(a.timeline.entries[0].kind, EntryKind::Shell);
    assert!(a.timeline.entries[0].title.contains("!echo hello_nexus [exit 0]"));
    assert!(a.timeline.entries[0].body.contains("hello_nexus"));
    let sh_txt = text(&render(&a));
    assert!(sh_txt.contains("▾ $ !echo hello_nexus [exit 0]"));
    assert!(sh_txt.contains("hello_nexus"));
}

#[test]
fn multiline_textarea_and_mouse_scroll_and_click() {
    let mut a = app();
    type_str(&mut a, "line one");
    a.handle_key(shift_enter());
    type_str(&mut a, "line two");
    a.handle_key(alt_enter());
    type_str(&mut a, "line three");
    assert_eq!(a.input_area.lines, vec!["line one", "line two", "line three"]);
    assert_eq!(a.input, "line one\nline two\nline three");

    // Cursor navigation across lines
    a.handle_key(key(KeyCode::Up));
    a.handle_key(key(KeyCode::Home));
    a.handle_key(ch('>'));
    assert_eq!(a.input_area.lines[1], ">line two");

    // Submit multiline prompt on Enter
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SendPrompt("line one\n>line two\nline three".into())]);

    // Mouse wheel scroll & click to fold/unfold collapsible Thinking card
    a.timeline.push(Entry::new(EntryKind::Thinking, "step 1\nstep 2"));
    a.timeline.push(Entry::new(EntryKind::Text, "final answer"));
    assert!(a.timeline.entries[0].collapsed);
    let _ = render(&a);

    let (start_y, _, _) = a.ui_regions.borrow().timeline_entries[0];
    a.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: start_y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!a.timeline.entries[0].collapsed);

    // Mouse scroll up/down moves timeline selection
    a.timeline_sel = 1;
    a.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(a.timeline_sel, 0);

    // Mouse click on `/` popup item executes the clicked command
    a.handle_key(ch('/'));
    let _ = render(&a);
    let first_popup_row = a.ui_regions.borrow().popup_rows[0].0;
    a.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: first_popup_row,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(a.overlay, Some(Overlay::Sessions));
}

#[test]
fn harness_selection_allowed_in_draft_and_locked_after_session_started() {
    let mut a = app();
    // Startup is a New Session draft: active_session_id does NOT fall back to foreground_id.
    assert!(a.new_session_draft);
    assert!(!a.is_session_started());
    assert_eq!(a.active_session_id(), None);

    // 1. In draft, Tab cycles harness (`claude-code` -> `codex`) without creating a session yet
    a.handle_key(key(KeyCode::Tab));
    assert_eq!(a.harness, "codex");
    assert!(a.new_session_draft);
    assert!(!a.is_session_started());
    assert_eq!(a.take_actions(), vec![Action::RefreshHarnessCommands]);

    // 2. `/harness antigravity` switches harness in draft
    type_str(&mut a, "/harness antigravity");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.harness, "antigravity");
    assert_eq!(a.take_actions(), vec![Action::RefreshHarnessCommands]);

    // 3. Opening an existing session via `/sessions` locks the harness
    type_str(&mut a, "/sessions");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, Some(Overlay::Sessions));
    assert_eq!(a.take_actions(), vec![Action::RefreshSessions]);

    // Select `cursor` session (index 2) and press Enter
    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, None);
    assert_eq!(a.harness, "cursor");
    assert!(!a.new_session_draft);
    assert!(a.is_session_started());
    assert_eq!(a.active_session_id(), Some("asd-3".into()));
    let actions = a.take_actions();
    assert!(matches!(actions.as_slice(), [Action::OpenSession(s)] if s.harness_session_id == "cu-1"));

    // 4. Once started, Tab / `/harness` / `switch_harness` / palette are locked!
    a.handle_key(key(KeyCode::Tab));
    assert_eq!(a.harness, "cursor");
    assert!(a.take_actions().is_empty());

    a.switch_harness("codex", None);
    assert_eq!(a.harness, "cursor");
    assert!(a.status.contains("locked"));
    assert!(a.take_actions().is_empty());

    type_str(&mut a, "/harness codex");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.harness, "cursor");
    assert!(a.status.contains("locked"));
    assert!(a.take_actions().is_empty());

    assert!(!a.all_slash_commands().iter().any(|c| c.name == "harness"));
    assert!(!a.palette_items().iter().any(|c| c.name.starts_with("harness")));

    // 5. `/new` resets back to draft where harness CAN be selected again
    type_str(&mut a, "/new");
    a.handle_key(key(KeyCode::Enter));
    assert!(a.new_session_draft);
    assert!(!a.is_session_started());
    assert_eq!(a.active_session_id(), None);
    assert!(a.take_actions().is_empty());

    type_str(&mut a, "/harness codex");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.harness, "codex");
    assert_eq!(a.take_actions(), vec![Action::RefreshHarnessCommands]);
}

#[test]
fn ctrl_overlays_toggle_load_and_render_in_both_themes() {
    let cases: [(char, Overlay, &str, Action); 5] = [
        ('b', Overlay::Board, "PROGRESS BOARD", Action::RefreshBoard),
        ('l', Overlay::Subagents, "SUBAGENTS", Action::RefreshSubagents),
        ('d', Overlay::Diff, "WORKTREE DIFF", Action::RefreshBoard),
        ('r', Overlay::Routing, "ROUTING CONFIG", Action::LoadRouting),
        ('t', Overlay::Terminal, "INTERACTIVE RESUME", Action::LoadResume("asd-1".into())),
    ];
    for (c, ov, title, first) in cases {
        let mut a = app();
        a.current_asd = Some("asd-1".into());
        a.new_session_draft = false;
        a.set_theme_mode(ThemeMode::Monochrome);
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
    type_str(&mut a, "scope");
    a.handle_key(key(KeyCode::Enter));
    assert!(matches!(a.prompt.as_ref().unwrap().purpose, PromptPurpose::DebateProposal { .. }));
    type_str(&mut a, "use ratatui");
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
    type_str(&mut a, "go");
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
    a.set_theme_mode(ThemeMode::Monochrome);
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
    a.current_asd = Some("asd-1".into());
    a.new_session_draft = false;
    a.set_theme_mode(ThemeMode::Monochrome);
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
fn sessions_modal_keys_and_search() {
    let mut a = app();
    type_str(&mut a, "/s");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, Some(Overlay::Sessions));
    assert_eq!(a.take_actions(), vec![Action::RefreshSessions]);

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

    // Promote the unpromoted codex session
    a.handle_key(key(KeyCode::Down));
    a.handle_key(ch('p'));
    assert!(matches!(a.take_actions().as_slice(), [Action::Promote(s)] if s.harness_session_id == "cx-1"));
    a.handle_key(ch('n'));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::CreateSession { title: None }]);

    // Search & harness filter inside `/sessions` modal
    a.handle_key(ch('/'));
    type_str(&mut a, "cu-");
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
fn model_picker_switches_harness_in_draft_and_locks_harness_on_started_session() {
    let mut a = app();
    a.set_theme_mode(ThemeMode::Monochrome);
    a.models.insert("codex".into(), vec!["gpt-5".into(), "gpt-5-mini".into(), "o3".into()]);
    a.models.insert("claude-code".into(), vec!["claude-sonnet-4".into(), "claude-opus-4".into()]);

    // 1. In New Session draft, ModelPicker allows switching harness + model
    a.handle_key(ctrl('k'));
    assert_eq!(a.overlay, Some(Overlay::ModelPicker));
    let draft_picker_txt = text(&render(&a));
    assert!(draft_picker_txt.contains("claude-code") && draft_picker_txt.contains("codex"));
    a.handle_key(key(KeyCode::Tab)); // -> codex
    type_str(&mut a, "mini");
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
    assert_eq!(a.take_actions(), vec![Action::RefreshHarnessCommands]);

    // 2. Once a session is started, ModelPicker locks harness (hides other tabs) while allowing model changes
    a.current_asd = Some("asd-1".into());
    a.new_session_draft = false;
    a.harness = "claude-code".into();
    a.model = None;
    a.set_status("Session active");

    a.handle_key(ctrl('k'));
    assert_eq!(a.overlay, Some(Overlay::ModelPicker));
    let locked_txt = text(&render(&a));
    assert!(locked_txt.contains("claude-code"));
    assert!(locked_txt.contains("harness locked for started session"));
    assert!(!locked_txt.contains("codex"));
    assert!(!locked_txt.contains("opencode"));
    assert!(!locked_txt.contains("antigravity"));
    assert!(!locked_txt.contains("cursor"));

    a.handle_key(key(KeyCode::Tab));
    a.handle_key(key(KeyCode::Right));
    assert_eq!(a.picker_harness, 0);

    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.overlay, None);
    assert_eq!(a.harness, "claude-code");
    assert_eq!(a.model.as_deref(), Some("claude-sonnet-4"));
    assert!(a.take_actions().is_empty());
}

#[test]
fn auto_approve_toggle_and_permission_modal_flow() {
    let mut a = app();
    assert!(a.auto_approve);
    assert!(text(&render(&a)).contains("[auto:on]"));

    // Toggle off in draft (does not push SetAutoApprove yet since no active session)
    type_str(&mut a, "/auto-approve off");
    a.handle_key(key(KeyCode::Enter));
    assert!(!a.auto_approve);
    assert!(a.take_actions().is_empty());
    assert!(text(&render(&a)).contains("[auto:off]"));

    // Bind a started session and toggle via `/auto-approve`
    a.current_asd = Some("asd-1".into());
    a.new_session_draft = false;
    type_str(&mut a, "/auto-approve on");
    a.handle_key(key(KeyCode::Enter));
    assert!(a.auto_approve);
    assert_eq!(a.take_actions(), vec![Action::SetAutoApprove(true)]);

    type_str(&mut a, "/auto-approve");
    a.handle_key(key(KeyCode::Enter));
    assert!(!a.auto_approve);
    assert_eq!(a.take_actions(), vec![Action::SetAutoApprove(false)]);

    // Stream a PermissionRequest SSE event (also captures asd-... session_id if none)
    a.current_asd = None;
    a.timeline.begin_turn("run bash");
    let (tx, rx) = mpsc::channel();
    a.stream = Some(asd_nexus_cli::app::ActiveStream { handle: None, rx });
    let mut p = SseParser::new();
    let raw = concat!(
        "data: {\"kind\":\"permission_request\",\"sessionId\":\"asd-99\",\"turnId\":\"turn-1\",\"permissionRequest\":{\"requestId\":\"req-1\",\"toolName\":\"Bash\",\"toolKind\":\"execute\",\"toolInput\":\"{\\\"command\\\":\\\"ls\\\"}\",\"locations\":[\"src/main.rs\"],\"options\":[{\"optionId\":\"allow_once\",\"name\":\"Allow once\",\"kind\":\"allow_once\"},{\"optionId\":\"allow_always\",\"name\":\"Always allow\",\"kind\":\"allow_always\"},{\"optionId\":\"reject_once\",\"name\":\"Reject\",\"kind\":\"reject_once\"}]}}\n\n",
        "data: {\"kind\":\"permission_request\",\"turnId\":\"turn-1\",\"permissionRequest\":{\"requestId\":\"req-2\",\"toolName\":\"Edit\",\"options\":[]}}\n\n",
    );
    for m in p.feed(raw.as_bytes()) {
        tx.send(m).unwrap();
    }
    a.drain_stream();
    assert_eq!(a.current_asd.as_deref(), Some("asd-99"));
    assert_eq!(a.pending_permissions.len(), 2);

    // Render modal in both DarkColor & Monochrome
    let dark_txt = text(&render(&a));
    assert!(dark_txt.contains("PERMISSION APPROVAL"));
    assert!(dark_txt.contains("Bash"));
    assert!(dark_txt.contains("execute"));
    assert!(dark_txt.contains("src/main.rs"));
    assert!(dark_txt.contains("Allow once"));
    a.set_theme_mode(ThemeMode::Monochrome);
    assert_monochrome(&render(&a));

    // Press `y` on first request -> allow_once
    a.handle_key(ch('y'));
    assert_eq!(
        a.take_actions(),
        vec![Action::RespondPermission {
            turn_id: "turn-1".into(),
            request_id: "req-1".into(),
            outcome: "selected".into(),
            option_id: Some("allow_once".into()),
        }]
    );
    assert_eq!(a.pending_permissions.len(), 1);

    // Second request has empty options (fallback Allow / Reject): Down + Enter rejects ("cancelled")
    a.handle_key(key(KeyCode::Down));
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(
        a.take_actions(),
        vec![Action::RespondPermission {
            turn_id: "turn-1".into(),
            request_id: "req-2".into(),
            outcome: "cancelled".into(),
            option_id: None,
        }]
    );
    assert!(a.pending_permissions.is_empty());

    // Verify `a` (allow_always) sets auto_approve = true and `n` / `Esc` rejects
    a.pending_permissions.push_back((
        "turn-2".into(),
        PermissionRequestPayload {
            request_id: "req-3".into(),
            tool_name: "Write".into(),
            options: vec![
                PermissionOptionItem {
                    option_id: "allow_once".into(),
                    name: "Allow once".into(),
                    kind: "allow_once".into(),
                },
                PermissionOptionItem {
                    option_id: "allow_always".into(),
                    name: "Always allow".into(),
                    kind: "allow_always".into(),
                },
                PermissionOptionItem {
                    option_id: "reject_once".into(),
                    name: "Reject".into(),
                    kind: "reject_once".into(),
                },
            ],
            ..Default::default()
        },
    ));
    a.pending_permissions.push_back((
        "turn-2".into(),
        PermissionRequestPayload {
            request_id: "req-4".into(),
            tool_name: "Bash".into(),
            options: vec![PermissionOptionItem {
                option_id: "reject_once".into(),
                name: "Reject".into(),
                kind: "reject_once".into(),
            }],
            ..Default::default()
        },
    ));
    a.handle_key(ch('a'));
    assert!(a.auto_approve);
    assert_eq!(
        a.take_actions(),
        vec![Action::RespondPermission {
            turn_id: "turn-2".into(),
            request_id: "req-3".into(),
            outcome: "selected".into(),
            option_id: Some("allow_always".into()),
        }]
    );
    a.handle_key(ch('n'));
    assert_eq!(
        a.take_actions(),
        vec![Action::RespondPermission {
            turn_id: "turn-2".into(),
            request_id: "req-4".into(),
            outcome: "selected".into(),
            option_id: Some("reject_once".into()),
        }]
    );
    assert!(a.pending_permissions.is_empty());
}

#[test]
fn streaming_timeline_over_mpsc_and_cancel() {
    let mut a = app();
    a.set_theme_mode(ThemeMode::Monochrome);
    type_str(&mut a, "hello");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.take_actions(), vec![Action::SendPrompt("hello".into())]);

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
    assert!(s.contains("▸ Thinking") && s.contains("▸ tool · grep") && s.contains("STREAMING"));

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
        vec![EntryKind::User, EntryKind::Thinking, EntryKind::ToolUse, EntryKind::ToolResult, EntryKind::Text]
    );
    assert_eq!(a.timeline.entries[3].title, "grep");
    let buf = render(&a);
    let s = text(&buf);
    assert!(s.contains("Answer"));
    assert!(s.contains("1,234") && s.contains("$0.0123"));
    assert_monochrome(&buf);
}

#[test]
fn timeline_collapse_via_keys_and_error_badge() {
    let mut a = app();
    a.set_theme_mode(ThemeMode::Monochrome);
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
    a.set_theme_mode(ThemeMode::Monochrome);
    a.handle_key(key(KeyCode::F(1)));
    assert_eq!(a.overlay, Some(Overlay::Help));
    assert_monochrome(&render(&a));
    a.handle_key(key(KeyCode::Esc));
    a.handle_key(ctrl('q'));
    assert!(a.should_quit);
}

#[test]
fn critic_remediation_unpromoted_session_new_cancel_and_permission_interrupt() {
    let mut a = app();
    // 1. Unpromoted CLI session with model=None clears stale model and does NOT leak foreground_id
    a.model = Some("claude-opus-4".into());
    a.open_overlay(Overlay::Sessions);
    a.take_actions();
    a.handle_key(key(KeyCode::Down)); // select cx-1 (unpromoted, model=None)
    a.handle_key(key(KeyCode::Enter));
    assert!(a.current.is_some());
    assert!(a.current_asd.is_none());
    assert_eq!(a.harness, "codex");
    assert!(a.model.is_none(), "opening session with model=None must clear previous model");
    assert_eq!(
        a.active_session_id(),
        None,
        "unpromoted CLI session must not fall back to foreground_id"
    );
    let actions = a.take_actions();
    assert!(matches!(actions.as_slice(), [Action::OpenSession(_)]));

    // 2. `/new` while streaming cancels active stream and clears stream/streaming state
    a.timeline.begin_turn("running prompt");
    let (_tx, rx) = mpsc::channel();
    a.stream = Some(asd_nexus_cli::app::ActiveStream { handle: None, rx });
    assert!(a.is_streaming());
    type_str(&mut a, "/new");
    a.handle_key(key(KeyCode::Enter));
    assert!(a.stream.is_none(), "/new must drop old ActiveStream");
    assert!(!a.is_streaming(), "/new must clear streaming state");
    assert_eq!(a.take_actions(), vec![Action::CancelTurn]);

    // 3. Ctrl+X during open permission modal clears pending_permissions and cancels active turn
    a.timeline.begin_turn("perm turn");
    let (tx2, rx2) = mpsc::channel();
    a.stream = Some(asd_nexus_cli::app::ActiveStream { handle: None, rx: rx2 });
    a.pending_permissions.push_back((
        "turn-ctrlx".into(),
        PermissionRequestPayload {
            request_id: "req-ctrlx".into(),
            tool_name: "Bash".into(),
            ..Default::default()
        },
    ));
    a.handle_key(ctrl('x'));
    assert!(a.pending_permissions.is_empty(), "Ctrl+X must clear pending_permissions");
    assert_eq!(a.take_actions(), vec![Action::CancelTurn]);

    // 4. pending_permissions clears automatically when stream finishes / interrupts
    a.pending_permissions.push_back((
        "turn-done".into(),
        PermissionRequestPayload {
            request_id: "req-done".into(),
            tool_name: "Edit".into(),
            ..Default::default()
        },
    ));
    let mut p = SseParser::new();
    for m in p.feed(b"data: {\"kind\":\"status\",\"status\":\"interrupted\"}\n\n") {
        tx2.send(m).unwrap();
    }
    a.drain_stream();
    assert!(!a.is_streaming());
    assert!(a.stream.is_none());
    assert!(
        a.pending_permissions.is_empty(),
        "drain_stream on interrupted/done must clear stale pending_permissions"
    );
}
