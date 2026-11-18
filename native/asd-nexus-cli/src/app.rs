//! Interactive TUI application state, OpenCode slash/file/shell UX, and pure key/mouse handling.
//!
//! Key and mouse handling are pure: they mutate UI state and queue [`Action`]s,
//! which are executed against the sidecar by [`crate::exec`]. This keeps every
//! interaction path deterministically testable without a live server.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use asd_nexus_client::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::textarea::TextArea;
use crate::theme::{self, ThemeMode};
use crate::timeline::{Entry, Timeline};

pub const HARNESSES: [&str; 5] = ["claude-code", "codex", "opencode", "antigravity", "cursor"];
pub const PHASES: [SessionPhase; 4] = [
    SessionPhase::Planning,
    SessionPhase::Running,
    SessionPhase::Paused,
    SessionPhase::Done,
];
pub const TERMINALS: [TerminalKind; 3] =
    [TerminalKind::WindowsTerminal, TerminalKind::Powershell, TerminalKind::Cmd];
const POLL_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Projects,
    Sessions,
    Timeline,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Board,
    Subagents,
    Diff,
    Routing,
    Terminal,
    ModelPicker,
    Sessions,
    DirPicker,
    CommandPalette,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardPane {
    Tasks,
    Plan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptPurpose {
    Search,
    NewSessionTitle,
    Memo { session_id: String },
    DebateTopic,
    DebateProposal { topic: String },
    RejectReason { task_id: String },
    SubagentCustom { sub_session_id: String },
}

impl PromptPurpose {
    pub fn label(&self) -> &'static str {
        match self {
            PromptPurpose::Search => "Search sessions",
            PromptPurpose::NewSessionTitle => "New session title (optional)",
            PromptPurpose::Memo { .. } => "Session memo",
            PromptPurpose::DebateTopic => "Debate topic",
            PromptPurpose::DebateProposal { .. } => "Debate proposal",
            PromptPurpose::RejectReason { .. } => "Reject reason",
            PromptPurpose::SubagentCustom { .. } => "Custom subagent instruction",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub purpose: PromptPurpose,
    pub buffer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandBadge {
    Nexus,
    Harness(String),
}

impl CommandBadge {
    pub fn label(&self) -> String {
        match self {
            CommandBadge::Nexus => "[nexus]".into(),
            CommandBadge::Harness(h) => format!("[{h}]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandEntry {
    pub name: String,
    pub usage: String,
    pub description: String,
    pub badge: CommandBadge,
    pub is_builtin: bool,
    pub kind_label: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct UiRegions {
    pub popup_rect: Option<Rect>,
    /// `(terminal_row_y, item_index)` for active `/` or `@` popup.
    pub popup_rows: Vec<(u16, usize)>,
    pub timeline_rect: Option<Rect>,
    /// `(start_y, end_y_exclusive, entry_index)` for visible timeline entries.
    pub timeline_entries: Vec<(u16, u16, usize)>,
    pub input_rect: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    LoadInitial,
    RefreshProjects,
    SelectProject(String),
    RefreshSessions,
    OpenSession(Box<DiscoveredSessionItem>),
    CreateSession { title: Option<String> },
    Promote(Box<DiscoveredSessionItem>),
    Demote(String),
    Unlink(String),
    Delete(String),
    SaveMemo { session_id: String, memo: String },
    SetForeground(String),
    SetPhase { session_id: String, phase: SessionPhase },
    SetAutoApprove(bool),
    RespondPermission { turn_id: String, request_id: String, outcome: String, option_id: Option<String> },
    RefreshModels { force: bool },
    RefreshHarnessCommands,
    ListFiles { prefix: Option<String> },
    ListDirs { prefix: Option<String> },
    RunShell(String),
    SendPrompt(String),
    CancelTurn,
    RefreshBoard,
    LoadPlan,
    SyncPlan,
    Dispatch { mode: String },
    Debate { topic: String, proposal: String },
    RefreshSubagents,
    TailLogs(String),
    SubagentAction { sub_session_id: String, action: SubagentControlActionKind, reason: Option<String> },
    LoadDiff(String),
    Approve(String),
    Reject { task_id: String, reason: Option<String> },
    LoadRouting,
    SaveRouting(String),
    ResetRouting,
    LoadResume(String),
    LaunchTerminal { session_id: String },
}

pub struct ActiveStream {
    pub handle: Option<TurnStreamHandle>,
    pub rx: Receiver<StreamMessage>,
}

const BUILTIN_COMMANDS: &[(&str, &str, &str)] = &[
    ("sessions", "/sessions", "Open session explorer modal (alias: /s)"),
    ("s", "/s", "Open session explorer modal"),
    ("cd", "/cd [path]", "Switch project directory or open directory picker"),
    ("new", "/new [title]", "Start a new session draft (or create with title)"),
    ("models", "/models", "Open harness & model picker modal (Ctrl+K)"),
    ("harness", "/harness [name]", "Switch active harness (Tab, new session only)"),
    ("auto-approve", "/auto-approve [on|off]", "Toggle or set tool auto-approve mode"),
    ("board", "/board", "Open ASD progress board & PLAN.md (Ctrl+B)"),
    ("diff", "/diff", "Open worktree diff & review modal (Ctrl+D)"),
    ("subagents", "/subagents", "Open subagents & live log tail modal (Ctrl+L)"),
    ("routing", "/routing", "Open multi-harness routing config modal (Ctrl+R)"),
    ("terminal", "/terminal", "Open interactive resume / OS terminal modal (Ctrl+T)"),
    ("theme", "/theme", "Toggle DarkColor / Monochrome theme"),
    ("sidebar", "/sidebar", "Toggle left projects & sessions drawer"),
    ("clear", "/clear", "Clear current conversation timeline"),
    ("help", "/help", "Show keyboard & slash command reference (F1)"),
    ("exit", "/exit", "Quit ASD-Nexus CLI (alias: /quit, Ctrl+Q)"),
    ("quit", "/quit", "Quit ASD-Nexus CLI"),
];

pub struct App {
    pub project_root: String,
    pub harness: String,
    pub model: Option<String>,
    pub cli_session: Option<String>,
    pub server_label: String,

    pub theme_mode: ThemeMode,
    pub sidebar_open: bool,

    pub focus: Focus,
    pub overlay: Option<Overlay>,
    pub prompt: Option<Prompt>,
    pub confirm: Option<(String, Action)>,
    pub pending: Vec<Action>,
    pub status: String,
    pub should_quit: bool,

    pub projects: Vec<ProjectSummaryItem>,
    pub project_sel: usize,

    pub sessions: Vec<DiscoveredSessionItem>,
    pub foreground_id: Option<String>,
    pub session_sel: usize,
    pub search: String,
    /// None = all harnesses, else index into HARNESSES.
    pub harness_filter: Option<usize>,
    pub current: Option<DiscoveredSessionItem>,
    pub current_asd: Option<String>,
    pub new_session_draft: bool,
    pub auto_approve: bool,
    pub pending_permissions: VecDeque<(String, PermissionRequestPayload)>,
    pub permission_sel: usize,
    pub pending_cancel_handles: Vec<TurnStreamHandle>,

    pub models: std::collections::HashMap<String, Vec<String>>,
    pub picker_harness: usize,
    pub picker_query: String,
    pub picker_sel: usize,

    pub harness_commands: Vec<HarnessCommandItem>,
    pub slash_sel: usize,
    pub popup_dismissed: bool,

    pub file_candidates: Vec<FsListEntry>,
    pub file_sel: usize,
    pub file_popup_open: bool,
    pub last_file_prefix: Option<Option<String>>,

    pub dir_candidates: Vec<FsListEntry>,
    pub dir_query: String,
    pub dir_sel: usize,

    pub palette_query: String,
    pub palette_sel: usize,

    pub timeline: Timeline,
    pub timeline_sel: usize,
    pub timeline_scroll: Option<usize>,
    pub input: String,
    pub input_area: TextArea,
    pub stream: Option<ActiveStream>,

    pub board: Option<GetBoardResponse>,
    pub board_sel: usize,
    pub board_pane: BoardPane,
    pub plan: Option<GetPlanResponse>,
    pub plan_scroll: u16,

    pub subagents: Vec<SubagentRecordItem>,
    pub sub_sel: usize,
    pub logs: Vec<String>,

    pub diff_sel: usize,
    pub diff: Option<WorktreeDiffResponse>,
    pub diff_scroll: u16,

    pub routing: Option<serde_json::Value>,
    pub routing_editor: Option<TextArea>,
    pub routing_scroll: u16,

    pub resume: Option<GetResumeCommandResponse>,
    pub terminal_kind: usize,

    pub last_poll: Option<Instant>,
    pub ui_regions: RefCell<UiRegions>,
}

fn ctrl(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char(c)
}

fn nav(sel: &mut usize, len: usize, code: KeyCode) -> bool {
    if len == 0 {
        *sel = 0;
        return matches!(code, KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k'));
    }
    match code {
        KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(len - 1),
        KeyCode::Home => *sel = 0,
        KeyCode::End => *sel = len - 1,
        _ => return false,
    }
    true
}

impl App {
    pub fn new(project_root: impl Into<String>, harness: impl Into<String>, model: Option<String>) -> Self {
        let harness = harness.into();
        let picker_harness = HARNESSES.iter().position(|h| *h == harness).unwrap_or(0);
        theme::set_mode(ThemeMode::DarkColor);
        Self {
            project_root: project_root.into(),
            harness,
            model,
            cli_session: None,
            server_label: String::new(),
            theme_mode: ThemeMode::DarkColor,
            sidebar_open: false,
            focus: Focus::Input,
            overlay: None,
            prompt: None,
            confirm: None,
            pending: vec![Action::LoadInitial],
            status: "Ready · / commands · @ files · ! shell · Ctrl+P palette".into(),
            should_quit: false,
            projects: vec![],
            project_sel: 0,
            sessions: vec![],
            foreground_id: None,
            session_sel: 0,
            search: String::new(),
            harness_filter: None,
            current: None,
            current_asd: None,
            new_session_draft: true,
            auto_approve: true,
            pending_permissions: VecDeque::new(),
            permission_sel: 0,
            pending_cancel_handles: Vec::new(),
            models: Default::default(),
            picker_harness,
            picker_query: String::new(),
            picker_sel: 0,
            harness_commands: vec![],
            slash_sel: 0,
            popup_dismissed: false,
            file_candidates: vec![],
            file_sel: 0,
            file_popup_open: false,
            last_file_prefix: None,
            dir_candidates: vec![],
            dir_query: String::new(),
            dir_sel: 0,
            palette_query: String::new(),
            palette_sel: 0,
            timeline: Timeline::default(),
            timeline_sel: 0,
            timeline_scroll: None,
            input: String::new(),
            input_area: TextArea::new(),
            stream: None,
            board: None,
            board_sel: 0,
            board_pane: BoardPane::Tasks,
            plan: None,
            plan_scroll: 0,
            subagents: vec![],
            sub_sel: 0,
            logs: vec![],
            diff_sel: 0,
            diff: None,
            diff_scroll: 0,
            routing: None,
            routing_editor: None,
            routing_scroll: 0,
            resume: None,
            terminal_kind: 1,
            last_poll: None,
            ui_regions: RefCell::new(UiRegions::default()),
        }
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.pending)
    }

    fn push(&mut self, a: Action) {
        self.pending.push(a);
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
        theme::set_mode(mode);
        self.set_status(format!("Theme: {}", mode.name()));
    }

    pub fn toggle_theme(&mut self) -> ThemeMode {
        let next = self.theme_mode.toggle();
        self.set_theme_mode(next);
        next
    }

    pub fn is_home(&self) -> bool {
        self.timeline.entries.is_empty()
    }

    pub fn is_streaming(&self) -> bool {
        self.stream.is_some() || self.timeline.streaming
    }

    pub fn is_session_started(&self) -> bool {
        !self.new_session_draft
            || self.current.is_some()
            || self.current_asd.is_some()
            || self.cli_session.is_some()
            || !self.timeline.entries.is_empty()
            || self.is_streaming()
    }

    pub fn cancel_active_stream(&mut self) {
        if let Some(mut st) = self.stream.take() {
            if let Some(h) = st.handle.take() {
                self.pending_cancel_handles.push(h);
            }
            self.push(Action::CancelTurn);
        }
        self.timeline.streaming = false;
    }

    pub fn start_new_session_draft(&mut self) {
        self.cancel_active_stream();
        self.current = None;
        self.current_asd = None;
        self.cli_session = None;
        self.new_session_draft = true;
        self.timeline.clear();
        self.timeline_sel = 0;
        self.timeline_scroll = None;
        self.resume = None;
        self.pending_permissions.clear();
        self.permission_sel = 0;
        self.set_status("New session — choose harness & model, then send a prompt");
    }

    pub fn set_auto_approve(&mut self, enabled: bool) {
        self.auto_approve = enabled;
        if self.active_session_id().is_some() {
            self.push(Action::SetAutoApprove(enabled));
        }
        self.set_status(format!(
            "Auto-approve: {}",
            if enabled { "ON" } else { "OFF (manual tool approval)" }
        ));
    }

    /// ASD session used for board / subagents / turns.
    pub fn active_session_id(&self) -> Option<String> {
        if let Some(ref id) = self.current_asd {
            return Some(id.clone());
        }
        if let Some(ref id) = self.cli_session {
            return Some(id.clone());
        }
        if self.new_session_draft || self.current.is_some() {
            return None;
        }
        self.foreground_id.clone()
    }

    // --------------------------------------------------------- harness switch

    /// Switch harness manually (`Tab`, `/harness`, `/models`).
    /// Allowed only on an unstarted New Session draft; locked once a session is started.
    pub fn switch_harness(&mut self, new_harness: &str, new_model: Option<String>) {
        let changed = self.harness != new_harness;
        if self.is_session_started() {
            if changed {
                self.set_status("Harness is locked for started session; run /new to pick another harness");
                return;
            }
            self.model = new_model;
            self.picker_harness = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
            self.set_status(format!(
                "Harness {} · model {}",
                self.harness,
                self.model.as_deref().unwrap_or("(default)")
            ));
            return;
        }
        self.harness = new_harness.to_string();
        self.model = new_model;
        self.picker_harness = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
        if changed {
            self.harness_commands.clear();
            self.push(Action::RefreshHarnessCommands);
            self.set_status(format!(
                "Selected {} · model {} (new session)",
                self.harness,
                self.model.as_deref().unwrap_or("(default)")
            ));
        } else {
            self.set_status(format!(
                "Harness {} · model {}",
                self.harness,
                self.model.as_deref().unwrap_or("(default)")
            ));
        }
    }

    // ---------------------------------------------------------------- sessions

    pub fn filtered_sessions(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| match self.harness_filter {
                Some(h) => s.harness == HARNESSES[h],
                None => true,
            })
            .filter(|(_, s)| {
                tokens.is_empty()
                    || tokens.iter().all(|tok| {
                        [
                            Some(&s.harness),
                            Some(&s.harness_session_id),
                            s.summary.as_ref(),
                            s.model.as_ref(),
                            s.asd_session_id.as_ref(),
                            s.asd_phase.as_ref(),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|f| f.to_lowercase().contains(tok))
                    })
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_session(&self) -> Option<&DiscoveredSessionItem> {
        self.filtered_sessions().get(self.session_sel).map(|&i| &self.sessions[i])
    }

    pub fn set_sessions(&mut self, resp: GetSessionsResponse) {
        self.sessions = resp.sessions;
        self.foreground_id = resp.active_session_id;
        let n = self.filtered_sessions().len();
        self.session_sel = self.session_sel.min(n.saturating_sub(1));
    }

    // ------------------------------------------------------------------ models

    pub fn picker_models(&self) -> Vec<String> {
        let q = self.picker_query.to_lowercase();
        let mut v: Vec<String> = self
            .models
            .get(HARNESSES[self.picker_harness])
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|m| q.is_empty() || m.to_lowercase().contains(&q))
            .collect();
        v.insert(0, "(default)".into());
        v
    }

    // ------------------------------------------------ slash & command palette

    pub fn all_slash_commands(&self) -> Vec<SlashCommandEntry> {
        let started = self.is_session_started();
        let mut out: Vec<SlashCommandEntry> = BUILTIN_COMMANDS
            .iter()
            .filter(|(name, _, _)| !(started && *name == "harness"))
            .map(|(name, usage, desc)| SlashCommandEntry {
                name: (*name).to_string(),
                usage: (*usage).to_string(),
                description: (*desc).to_string(),
                badge: CommandBadge::Nexus,
                is_builtin: true,
                kind_label: "builtin",
            })
            .collect();

        for hc in &self.harness_commands {
            let name = hc.name.trim_start_matches('/').to_string();
            if name.is_empty() {
                continue;
            }
            let kind_label = match hc.kind {
                HarnessCommandKind::Command => "command",
                HarnessCommandKind::Skill => "skill",
                HarnessCommandKind::Workflow => "workflow",
            };
            out.push(SlashCommandEntry {
                usage: format!("/{name}"),
                name,
                description: hc.description.clone(),
                badge: CommandBadge::Harness(hc.harness.clone()),
                is_builtin: false,
                kind_label,
            });
        }
        out
    }

    fn current_slash_query(&self) -> Option<String> {
        if self.popup_dismissed
            || !self.pending_permissions.is_empty()
            || self.overlay.is_some()
            || self.prompt.is_some()
            || self.confirm.is_some()
        {
            return None;
        }
        if self.input_area.lines.len() != 1 {
            return None;
        }
        let trimmed = self.input_area.lines[0].trim_start();
        if !trimmed.starts_with('/') {
            return None;
        }
        let after = &trimmed[1..];
        if after.contains(char::is_whitespace) {
            return None;
        }
        Some(after.to_lowercase())
    }

    pub fn is_slash_popup_active(&self) -> bool {
        self.current_slash_query().is_some() && !self.filtered_slash_commands().is_empty()
    }

    pub fn filtered_slash_commands(&self) -> Vec<SlashCommandEntry> {
        let Some(q) = self.current_slash_query() else {
            return Vec::new();
        };
        let all = self.all_slash_commands();
        if q.is_empty() {
            // Hide duplicate short alias `/s` and `/quit` when query is empty to keep list tidy.
            return all.into_iter().filter(|c| c.name != "s" && c.name != "quit").collect();
        }
        let mut prefix_matches = Vec::new();
        let mut substring_matches = Vec::new();
        for c in all {
            let name_l = c.name.to_lowercase();
            let desc_l = c.description.to_lowercase();
            if name_l.starts_with(&q) {
                prefix_matches.push(c);
            } else if name_l.contains(&q) || desc_l.contains(&q) {
                substring_matches.push(c);
            }
        }
        prefix_matches.extend(substring_matches);
        prefix_matches
    }

    pub fn palette_items(&self) -> Vec<SlashCommandEntry> {
        let mut all = self.all_slash_commands();
        all.retain(|c| c.name != "s" && c.name != "quit");
        if !self.is_session_started() {
            for h in HARNESSES {
                all.push(SlashCommandEntry {
                    name: format!("harness {h}"),
                    usage: format!("/harness {h}"),
                    description: format!("Switch active harness to {h} (new session)"),
                    badge: CommandBadge::Nexus,
                    is_builtin: true,
                    kind_label: "harness",
                });
            }
        }
        let q = self.palette_query.trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        let tokens: Vec<&str> = q.split_whitespace().collect();
        all.into_iter()
            .filter(|c| {
                let hay = format!("{} {} {} {}", c.name, c.usage, c.description, c.badge.label()).to_lowercase();
                tokens.iter().all(|t| hay.contains(t))
            })
            .collect()
    }

    // -------------------------------------------------------- @file & /cd dir

    pub fn is_file_popup_active(&self) -> bool {
        self.file_popup_open
            && self.pending_permissions.is_empty()
            && self.overlay.is_none()
            && self.prompt.is_none()
            && self.confirm.is_none()
            && self.input_area.active_at_token().is_some()
            && !self.filtered_files().is_empty()
    }

    pub fn filtered_files(&self) -> Vec<FsListEntry> {
        let q = self
            .input_area
            .active_at_token()
            .map(|(_, _, q)| q.to_lowercase())
            .unwrap_or_default();
        self.file_candidates
            .iter()
            .filter(|e| q.is_empty() || e.path.to_lowercase().contains(&q) || e.name.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Combined directory list for `/cd` modal: known `projects` + `dir_candidates` (`fs_list(Dirs)`).
    pub fn dir_picker_items(&self) -> Vec<(String, String)> {
        let q = self.dir_query.trim().to_lowercase();
        let mut out: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for p in &self.projects {
            if q.is_empty() || p.project_root.to_lowercase().contains(&q) {
                if seen.insert(p.project_root.clone()) {
                    let count = p.session_count.map(|c| format!("{c} sessions")).unwrap_or_else(|| "project".into());
                    out.push((p.project_root.clone(), format!("[project] {count}")));
                }
            }
        }
        for d in &self.dir_candidates {
            if q.is_empty() || d.path.to_lowercase().contains(&q) || d.name.to_lowercase().contains(&q) {
                let full = self.resolve_dir_path(&d.path);
                if seen.insert(full.clone()) {
                    out.push((full, format!("[dir] {}", d.path)));
                }
            }
        }
        out
    }

    pub fn resolve_dir_path(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        let p = Path::new(trimmed);
        if p.is_absolute() || trimmed.contains(":/") || trimmed.contains(":\\") || trimmed.starts_with('/') {
            trimmed.replace('\\', "/")
        } else {
            let base = self.project_root.trim_end_matches(['/', '\\']);
            format!("{base}/{}", trimmed.trim_start_matches(['/', '\\'])).replace('\\', "/")
        }
    }

    // ------------------------------------------------------------- local shell

    /// Execute a `!shell` command in `project_root` and append an [`EntryKind::Shell`] card to the timeline.
    pub fn execute_shell(&mut self, cmd: &str) {
        let mut command = if cfg!(windows) {
            let mut c = ProcessCommand::new("cmd");
            c.args(["/C", cmd]);
            c
        } else {
            let mut c = ProcessCommand::new("sh");
            c.args(["-c", cmd]);
            c
        };
        let root_path = Path::new(&self.project_root);
        if root_path.is_dir() {
            command.current_dir(root_path);
        }
        match command.output() {
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut combined = String::new();
                if !stdout.trim_end().is_empty() {
                    combined.push_str(stdout.trim_end());
                }
                if !stderr.trim_end().is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(stderr.trim_end());
                }
                if combined.is_empty() {
                    combined = format!("(no output, exit code {code})");
                }
                self.timeline.push(Entry::shell(cmd, combined, code));
                self.timeline_sel = self.timeline.entries.len().saturating_sub(1);
                self.timeline_scroll = None;
                self.set_status(format!("Shell !{cmd} (exit {code})"));
            }
            Err(e) => {
                self.timeline.push(Entry::shell(cmd, format!("shell error: {e}"), -1));
                self.timeline_sel = self.timeline.entries.len().saturating_sub(1);
                self.timeline_scroll = None;
                self.set_status(format!("Shell failed: {e}"));
            }
        }
    }

    // ---------------------------------------------------------------- streaming

    /// Drain pending SSE messages into the timeline.
    pub fn drain_stream(&mut self) {
        let Some(st) = self.stream.as_ref() else { return };
        let mut msgs = Vec::new();
        let mut closed = false;
        loop {
            match st.rx.try_recv() {
                Ok(m) => {
                    let done = m == StreamMessage::Done;
                    msgs.push(m);
                    if done {
                        closed = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        for m in &msgs {
            if let StreamMessage::Event(evt) = m {
                if self.current_asd.is_none() {
                    if let Some(sid) = evt.session_id.as_deref() {
                        if sid.starts_with("asd-") {
                            self.current_asd = Some(sid.to_string());
                            self.new_session_draft = false;
                        }
                    }
                }
                if evt.kind == OrchestratorEventKind::PermissionRequest {
                    if let Some(perm) = evt.permission_request.clone() {
                        let tid = evt
                            .turn_id
                            .clone()
                            .or_else(|| self.timeline.turn_id.clone())
                            .unwrap_or_default();
                        self.pending_permissions.push_back((tid, perm));
                        self.permission_sel = 0;
                    }
                }
            }
            self.timeline.apply(m);
        }
        if !msgs.is_empty() {
            self.timeline_sel = self.timeline.entries.len().saturating_sub(1);
            self.timeline_scroll = None;
        }
        if closed || !self.timeline.streaming {
            self.timeline.end_turn(Some("stream closed"));
            self.stream = None;
            self.pending_permissions.clear();
            self.permission_sel = 0;
            self.set_status("Turn finished");
        }
    }

    // ------------------------------------------------------------------ polling

    /// Queue 1s refresh actions while monitoring overlays are open.
    pub fn tick(&mut self, now: Instant) {
        if self.last_poll.is_some_and(|t| now.duration_since(t) < POLL_EVERY) {
            return;
        }
        self.last_poll = Some(now);
        match self.overlay {
            Some(Overlay::Board) => self.push(Action::RefreshBoard),
            Some(Overlay::Subagents) => {
                self.push(Action::RefreshSubagents);
                if let Some(s) = self.subagents.get(self.sub_sel) {
                    let id = s.sub_session_id.clone();
                    self.push(Action::TailLogs(id));
                }
            }
            _ => {}
        }
    }

    // --------------------------------------------------------------- overlays

    pub fn open_overlay(&mut self, o: Overlay) {
        if self.overlay == Some(o) {
            self.overlay = None;
            self.routing_editor = None;
            return;
        }
        self.overlay = Some(o);
        self.routing_editor = None;
        match o {
            Overlay::Board => {
                self.push(Action::RefreshBoard);
                self.push(Action::LoadPlan);
            }
            Overlay::Subagents => self.push(Action::RefreshSubagents),
            Overlay::Diff => {
                self.diff = None;
                self.push(Action::RefreshBoard);
            }
            Overlay::Routing => self.push(Action::LoadRouting),
            Overlay::Terminal => {
                self.resume = None;
                match self.active_session_id() {
                    Some(id) => self.push(Action::LoadResume(id)),
                    None => self.set_status("No ASD session selected (promote or create one first)"),
                }
            }
            Overlay::ModelPicker => {
                self.picker_harness = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
                self.picker_query.clear();
                self.picker_sel = 0;
                if self.models.is_empty() {
                    self.push(Action::RefreshModels { force: false });
                }
            }
            Overlay::Sessions => {
                self.push(Action::RefreshSessions);
            }
            Overlay::DirPicker => {
                self.dir_query.clear();
                self.dir_sel = 0;
                self.push(Action::ListDirs { prefix: None });
            }
            Overlay::CommandPalette => {
                self.palette_query.clear();
                self.palette_sel = 0;
            }
            Overlay::Help => {}
        }
    }

    pub fn board_tasks(&self) -> &[BoardTaskItem] {
        self.board.as_ref().map(|b| b.tasks.as_slice()).unwrap_or(&[])
    }

    // -------------------------------------------------------------- key input

    pub fn handle_key(&mut self, k: KeyEvent) {
        if !self.pending_permissions.is_empty() {
            if ctrl(&k, 'c') || ctrl(&k, 'q') {
                if self.is_streaming() {
                    self.push(Action::CancelTurn);
                }
                self.should_quit = true;
                return;
            }
            if ctrl(&k, 'x') {
                self.pending_permissions.clear();
                self.permission_sel = 0;
                if self.is_streaming() {
                    self.push(Action::CancelTurn);
                } else {
                    self.set_status("No active turn");
                }
                return;
            }
            self.handle_permission_key(k);
            return;
        }
        if let Some((_, action)) = self.confirm.take() {
            if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                self.push(action);
            } else {
                self.set_status("Cancelled");
            }
            return;
        }
        if self.prompt.is_some() {
            self.handle_prompt_key(k);
            return;
        }

        // Global shortcuts.
        if ctrl(&k, 'c') || ctrl(&k, 'q') {
            if self.is_streaming() {
                self.push(Action::CancelTurn);
            }
            self.should_quit = true;
            return;
        }
        let overlay_key = [
            ('b', Overlay::Board),
            ('l', Overlay::Subagents),
            ('d', Overlay::Diff),
            ('r', Overlay::Routing),
            ('t', Overlay::Terminal),
            ('k', Overlay::ModelPicker),
            ('p', Overlay::CommandPalette),
        ];
        for (c, o) in overlay_key {
            if ctrl(&k, c) {
                self.open_overlay(o);
                return;
            }
        }
        if ctrl(&k, 'x') {
            if self.is_streaming() {
                self.push(Action::CancelTurn);
            } else {
                self.set_status("No active turn");
            }
            return;
        }
        if ctrl(&k, 'e') {
            self.timeline.toggle_all();
            return;
        }
        if k.code == KeyCode::F(1) {
            self.open_overlay(Overlay::Help);
            return;
        }

        if let Some(o) = self.overlay {
            self.handle_overlay_key(o, k);
            return;
        }

        // Tab handling:
        // - In Input focus with active `/` or `@` popup: autocomplete popup item.
        // - In Input focus without sidebar: cycle 5 harnesses (creating new session).
        // - When sidebar is open or focus is on Projects/Sessions/Timeline: cycle focus panes.
        if k.code == KeyCode::Tab || k.code == KeyCode::BackTab {
            if self.focus == Focus::Input && (self.is_slash_popup_active() || self.is_file_popup_active()) {
                self.handle_input_key(k);
                return;
            }
            if self.focus == Focus::Input && !self.sidebar_open {
                if self.is_session_started() {
                    self.set_status("Harness is locked for started session; run /new to pick another harness");
                    return;
                }
                let i = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
                let next = if k.code == KeyCode::Tab {
                    (i + 1) % HARNESSES.len()
                } else {
                    (i + HARNESSES.len() - 1) % HARNESSES.len()
                };
                self.switch_harness(HARNESSES[next], None);
                return;
            }
            let order = [Focus::Projects, Focus::Sessions, Focus::Timeline, Focus::Input];
            let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
            let n = if k.code == KeyCode::Tab { i + 1 } else { i + 3 };
            self.focus = order[n % 4];
            return;
        }

        match self.focus {
            Focus::Projects => self.handle_projects_key(k),
            Focus::Sessions => self.handle_sessions_key(k),
            Focus::Timeline => self.handle_timeline_key(k),
            Focus::Input => self.handle_input_key(k),
        }
    }

    // ------------------------------------------------------------- mouse input

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if !self.pending_permissions.is_empty() || self.confirm.is_some() || self.prompt.is_some() {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.scroll_up(),
            MouseEventKind::ScrollDown => self.scroll_down(),
            MouseEventKind::Down(MouseButton::Left) => self.click_at(m.column, m.row),
            _ => {}
        }
    }

    fn scroll_up(&mut self) {
        if self.is_slash_popup_active() {
            self.slash_sel = self.slash_sel.saturating_sub(1);
            return;
        }
        if self.is_file_popup_active() {
            self.file_sel = self.file_sel.saturating_sub(1);
            return;
        }
        if let Some(ov) = self.overlay {
            match ov {
                Overlay::CommandPalette => self.palette_sel = self.palette_sel.saturating_sub(1),
                Overlay::Sessions => self.session_sel = self.session_sel.saturating_sub(1),
                Overlay::DirPicker => self.dir_sel = self.dir_sel.saturating_sub(1),
                Overlay::ModelPicker => self.picker_sel = self.picker_sel.saturating_sub(1),
                Overlay::Board => match self.board_pane {
                    BoardPane::Tasks => self.board_sel = self.board_sel.saturating_sub(1),
                    BoardPane::Plan => self.plan_scroll = self.plan_scroll.saturating_sub(2),
                },
                Overlay::Diff => self.diff_scroll = self.diff_scroll.saturating_sub(2),
                Overlay::Routing => self.routing_scroll = self.routing_scroll.saturating_sub(2),
                Overlay::Subagents => self.sub_sel = self.sub_sel.saturating_sub(1),
                Overlay::Terminal | Overlay::Help => {}
            }
            return;
        }
        if !self.timeline.entries.is_empty() {
            self.timeline_sel = self.timeline_sel.saturating_sub(1);
            self.timeline_scroll = Some(self.timeline_scroll.unwrap_or(0).saturating_sub(2));
        }
    }

    fn scroll_down(&mut self) {
        if self.is_slash_popup_active() {
            let max = self.filtered_slash_commands().len().saturating_sub(1);
            self.slash_sel = (self.slash_sel + 1).min(max);
            return;
        }
        if self.is_file_popup_active() {
            let max = self.filtered_files().len().saturating_sub(1);
            self.file_sel = (self.file_sel + 1).min(max);
            return;
        }
        if let Some(ov) = self.overlay {
            match ov {
                Overlay::CommandPalette => {
                    let max = self.palette_items().len().saturating_sub(1);
                    self.palette_sel = (self.palette_sel + 1).min(max);
                }
                Overlay::Sessions => {
                    let max = self.filtered_sessions().len().saturating_sub(1);
                    self.session_sel = (self.session_sel + 1).min(max);
                }
                Overlay::DirPicker => {
                    let max = self.dir_picker_items().len().saturating_sub(1);
                    self.dir_sel = (self.dir_sel + 1).min(max);
                }
                Overlay::ModelPicker => {
                    let max = self.picker_models().len().saturating_sub(1);
                    self.picker_sel = (self.picker_sel + 1).min(max);
                }
                Overlay::Board => match self.board_pane {
                    BoardPane::Tasks => {
                        let max = self.board_tasks().len().saturating_sub(1);
                        self.board_sel = (self.board_sel + 1).min(max);
                    }
                    BoardPane::Plan => self.plan_scroll = self.plan_scroll.saturating_add(2),
                },
                Overlay::Diff => self.diff_scroll = self.diff_scroll.saturating_add(2),
                Overlay::Routing => self.routing_scroll = self.routing_scroll.saturating_add(2),
                Overlay::Subagents => {
                    let max = self.subagents.len().saturating_sub(1);
                    self.sub_sel = (self.sub_sel + 1).min(max);
                }
                Overlay::Terminal | Overlay::Help => {}
            }
            return;
        }
        if !self.timeline.entries.is_empty() {
            let max = self.timeline.entries.len().saturating_sub(1);
            self.timeline_sel = (self.timeline_sel + 1).min(max);
            self.timeline_scroll = Some(self.timeline_scroll.unwrap_or(0).saturating_add(2));
        }
    }

    fn click_at(&mut self, col: u16, row: u16) {
        let regions = self.ui_regions.borrow().clone();

        // 1. Check active `/` or `@` popup click
        if self.is_slash_popup_active() {
            if let Some((_, idx)) = regions.popup_rows.iter().find(|(y, _)| *y == row) {
                self.slash_sel = *idx;
                self.submit_input();
                return;
            }
        }
        if self.is_file_popup_active() {
            if let Some((_, idx)) = regions.popup_rows.iter().find(|(y, _)| *y == row) {
                self.file_sel = *idx;
                self.complete_file_selection();
                return;
            }
        }

        if self.overlay.is_some() {
            return;
        }

        // 2. Check timeline collapsible entry click
        if let Some((_, _, idx)) = regions
            .timeline_entries
            .iter()
            .find(|(start_y, end_y, _)| row >= *start_y && row < *end_y)
        {
            self.timeline_sel = *idx;
            self.timeline.toggle(*idx);
            return;
        }

        // 3. Check input area click
        if let Some(r) = regions.input_rect {
            if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                self.focus = Focus::Input;
            }
        }
    }

    // -------------------------------------------------------------- prompts

    fn handle_prompt_key(&mut self, k: KeyEvent) {
        let Some(p) = self.prompt.as_mut() else { return };
        match k.code {
            KeyCode::Esc => {
                if p.purpose == PromptPurpose::Search {
                    self.search.clear();
                }
                self.prompt = None;
            }
            KeyCode::Backspace => {
                p.buffer.pop();
                if p.purpose == PromptPurpose::Search {
                    self.search = p.buffer.clone();
                    self.session_sel = 0;
                }
            }
            KeyCode::Char(c) => {
                p.buffer.push(c);
                if p.purpose == PromptPurpose::Search {
                    self.search = p.buffer.clone();
                    self.session_sel = 0;
                }
            }
            KeyCode::Enter => {
                let p = self.prompt.take().expect("prompt");
                let text = p.buffer.trim().to_string();
                let opt = (!text.is_empty()).then(|| text.clone());
                match p.purpose {
                    PromptPurpose::Search => self.search = text,
                    PromptPurpose::NewSessionTitle => {
                        self.new_session_draft = false;
                        self.push(Action::CreateSession { title: opt });
                    }
                    PromptPurpose::Memo { session_id } => {
                        self.push(Action::SaveMemo { session_id, memo: p.buffer })
                    }
                    PromptPurpose::DebateTopic => {
                        if let Some(topic) = opt {
                            self.prompt = Some(Prompt {
                                purpose: PromptPurpose::DebateProposal { topic },
                                buffer: String::new(),
                            });
                        }
                    }
                    PromptPurpose::DebateProposal { topic } => {
                        if let Some(proposal) = opt {
                            self.push(Action::Debate { topic, proposal });
                        }
                    }
                    PromptPurpose::RejectReason { task_id } => {
                        self.push(Action::Reject { task_id, reason: opt })
                    }
                    PromptPurpose::SubagentCustom { sub_session_id } => self.push(Action::SubagentAction {
                        sub_session_id,
                        action: SubagentControlActionKind::Custom,
                        reason: opt,
                    }),
                }
            }
            _ => {}
        }
    }

    fn open_prompt(&mut self, purpose: PromptPurpose, buffer: String) {
        self.prompt = Some(Prompt { purpose, buffer });
    }

    fn handle_projects_key(&mut self, k: KeyEvent) {
        if nav(&mut self.project_sel, self.projects.len(), k.code) {
            return;
        }
        match k.code {
            KeyCode::Enter => {
                if let Some(p) = self.projects.get(self.project_sel) {
                    let root = p.project_root.clone();
                    self.push(Action::SelectProject(root));
                }
            }
            KeyCode::Char('r') => self.push(Action::RefreshProjects),
            _ => {}
        }
    }

    fn require_asd(&mut self, s: &DiscoveredSessionItem) -> Option<String> {
        let id = s.asd_session_id.clone();
        if id.is_none() {
            self.set_status("Session is not promoted to ASD (press p to promote)");
        }
        id
    }

    fn handle_sessions_key(&mut self, k: KeyEvent) {
        let n = self.filtered_sessions().len();
        if nav(&mut self.session_sel, n, k.code) {
            return;
        }
        let sel = self.selected_session().cloned();
        match k.code {
            KeyCode::Enter => {
                if let Some(s) = sel {
                    // OpenSession adjusts harness to the session's harness without creating a new session!
                    self.harness = s.harness.clone();
                    self.model = s.model.clone();
                    self.auto_approve = s.auto_approve.unwrap_or(true);
                    self.new_session_draft = false;
                    self.current_asd = s.asd_session_id.clone();
                    self.current = Some(s.clone());
                    self.picker_harness = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
                    if self.overlay == Some(Overlay::Sessions) {
                        self.overlay = None;
                    }
                    self.push(Action::OpenSession(Box::new(s)));
                }
            }
            KeyCode::Char('n') => self.open_prompt(PromptPurpose::NewSessionTitle, String::new()),
            KeyCode::Char('/') => {
                let b = self.search.clone();
                self.open_prompt(PromptPurpose::Search, b);
            }
            KeyCode::Char('r') => self.push(Action::RefreshSessions),
            KeyCode::Char('h') => {
                self.harness_filter = match self.harness_filter {
                    None => Some(0),
                    Some(i) if i + 1 < HARNESSES.len() => Some(i + 1),
                    Some(_) => None,
                };
                self.session_sel = 0;
            }
            KeyCode::Char('H') => {
                let i = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
                let next = HARNESSES[(i + 1) % HARNESSES.len()];
                self.switch_harness(next, None);
            }
            KeyCode::Char('m') => self.open_overlay(Overlay::ModelPicker),
            _ => {
                let Some(s) = sel else { return };
                match k.code {
                    KeyCode::Char('p') => {
                        if s.is_promoted_to_asd {
                            self.set_status("Already promoted");
                        } else {
                            self.push(Action::Promote(Box::new(s)));
                        }
                    }
                    KeyCode::Char('d') => {
                        if let Some(id) = self.require_asd(&s) {
                            self.push(Action::Demote(id));
                        }
                    }
                    KeyCode::Char('u') => {
                        if let Some(id) = self.require_asd(&s) {
                            self.confirm = Some((format!("Unlink ASD session {id}?"), Action::Unlink(id)));
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Delete => {
                        if let Some(id) = self.require_asd(&s) {
                            self.confirm = Some((format!("Delete ASD session {id}?"), Action::Delete(id)));
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(id) = self.require_asd(&s) {
                            let memo = s.summary.clone().unwrap_or_default();
                            self.open_prompt(PromptPurpose::Memo { session_id: id }, memo);
                        }
                    }
                    KeyCode::Char('f') => {
                        if let Some(id) = self.require_asd(&s) {
                            self.push(Action::SetForeground(id));
                        }
                    }
                    KeyCode::Char('t') => {
                        if let Some(id) = self.require_asd(&s) {
                            let cur = s.asd_phase.as_deref().unwrap_or("planning");
                            let i = PHASES
                                .iter()
                                .position(|p| phase_name(*p) == cur)
                                .map(|i| (i + 1) % PHASES.len())
                                .unwrap_or(0);
                            self.push(Action::SetPhase { session_id: id, phase: PHASES[i] });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_timeline_key(&mut self, k: KeyEvent) {
        let n = self.timeline.entries.len();
        if nav(&mut self.timeline_sel, n, k.code) {
            self.timeline_scroll = None;
            return;
        }
        match k.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.timeline.toggle(self.timeline_sel),
            KeyCode::Char('i') | KeyCode::Esc => self.focus = Focus::Input,
            _ => {}
        }
    }

    // --------------------------------------------------------- input & popups

    fn sync_input_before_edit(&mut self) {
        if self.input != self.input_area.text() {
            self.input_area.set_text(&self.input);
        }
    }

    fn sync_input_after_edit(&mut self) {
        self.input = self.input_area.text();

        // Update `/` slash popup state
        if self.input_area.lines.len() == 1 && self.input_area.lines[0].trim_start().starts_with('/') {
            if self.input_area.lines[0].trim() == "/" {
                self.popup_dismissed = false;
            }
            let max = self.filtered_slash_commands().len().saturating_sub(1);
            self.slash_sel = self.slash_sel.min(max);
        } else {
            self.popup_dismissed = false;
            self.slash_sel = 0;
        }

        // Update `@` file popup state
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') && !trimmed.starts_with('!') {
            if let Some((_, _, query)) = self.input_area.active_at_token() {
                self.file_popup_open = true;
                let prefix = (!query.is_empty()).then_some(query);
                if self.last_file_prefix.as_ref() != Some(&prefix) {
                    self.last_file_prefix = Some(prefix.clone());
                    self.push(Action::ListFiles { prefix });
                }
                let max = self.filtered_files().len().saturating_sub(1);
                self.file_sel = self.file_sel.min(max);
            } else {
                self.file_popup_open = false;
                self.last_file_prefix = None;
                self.file_sel = 0;
            }
        } else {
            self.file_popup_open = false;
            self.last_file_prefix = None;
            self.file_sel = 0;
        }
    }

    fn complete_slash_selection(&mut self) {
        let cmds = self.filtered_slash_commands();
        if let Some(cmd) = cmds.get(self.slash_sel) {
            let new_text = if matches!(cmd.name.as_str(), "cd" | "harness" | "new") || !cmd.is_builtin {
                format!("/{} ", cmd.name)
            } else {
                format!("/{}", cmd.name)
            };
            self.input_area.set_text(&new_text);
            self.sync_input_after_edit();
        }
    }

    fn complete_file_selection(&mut self) -> bool {
        let files = self.filtered_files();
        let Some(entry) = files.get(self.file_sel).cloned() else {
            return false;
        };
        let Some((start_col, end_col, _)) = self.input_area.active_at_token() else {
            return false;
        };
        let replacement = format!("@{} ", entry.path);
        self.input_area.replace_current_line_span(start_col, end_col, &replacement);
        self.input = self.input_area.text();
        self.file_popup_open = false;
        self.file_sel = 0;
        true
    }

    fn handle_input_key(&mut self, k: KeyEvent) {
        self.sync_input_before_edit();

        // Multiline newline: Shift+Enter, Alt+Enter, or Ctrl+J
        if (k.code == KeyCode::Enter
            && (k.modifiers.contains(KeyModifiers::SHIFT) || k.modifiers.contains(KeyModifiers::ALT)))
            || ctrl(&k, 'j')
        {
            self.input_area.newline();
            self.sync_input_after_edit();
            return;
        }

        // `/` slash popup navigation / completion
        if self.is_slash_popup_active() {
            match k.code {
                KeyCode::Up => {
                    self.slash_sel = self.slash_sel.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    let max = self.filtered_slash_commands().len().saturating_sub(1);
                    self.slash_sel = (self.slash_sel + 1).min(max);
                    return;
                }
                KeyCode::Tab => {
                    self.complete_slash_selection();
                    return;
                }
                KeyCode::Esc => {
                    self.popup_dismissed = true;
                    return;
                }
                KeyCode::Enter => {
                    self.submit_input();
                    return;
                }
                _ => {}
            }
        }

        // `@` file popup navigation / completion
        if self.is_file_popup_active() {
            match k.code {
                KeyCode::Up => {
                    self.file_sel = self.file_sel.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    let max = self.filtered_files().len().saturating_sub(1);
                    self.file_sel = (self.file_sel + 1).min(max);
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if self.complete_file_selection() {
                        return;
                    }
                }
                KeyCode::Esc => {
                    self.file_popup_open = false;
                    return;
                }
                _ => {}
            }
        }

        match k.code {
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                self.input_area.backspace();
                self.sync_input_after_edit();
            }
            KeyCode::Delete => {
                self.input_area.delete();
                self.sync_input_after_edit();
            }
            KeyCode::Left => {
                self.input_area.left();
                self.sync_input_after_edit();
            }
            KeyCode::Right => {
                self.input_area.right();
                self.sync_input_after_edit();
            }
            KeyCode::Up => {
                self.input_area.up();
                self.sync_input_after_edit();
            }
            KeyCode::Down => {
                self.input_area.down();
                self.sync_input_after_edit();
            }
            KeyCode::Home => {
                self.input_area.home();
                self.sync_input_after_edit();
            }
            KeyCode::End => {
                self.input_area.end();
                self.sync_input_after_edit();
            }
            KeyCode::Esc => self.focus = Focus::Timeline,
            KeyCode::Char(c) => {
                self.input_area.insert(c);
                self.sync_input_after_edit();
            }
            _ => {}
        }
    }

    fn clear_input(&mut self) {
        self.input_area.clear();
        self.input.clear();
        self.popup_dismissed = false;
        self.slash_sel = 0;
        self.file_popup_open = false;
        self.file_sel = 0;
        self.last_file_prefix = None;
    }

    fn submit_input(&mut self) {
        self.sync_input_before_edit();
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }

        // 1. Slash command (`/cmd ...`)
        if text.starts_with('/') && !text.contains('\n') {
            let selected = if self.is_slash_popup_active() {
                self.filtered_slash_commands().get(self.slash_sel).cloned()
            } else {
                None
            };
            self.clear_input();
            self.execute_slash_command(&text, selected);
            return;
        }

        // 2. Shell command (`!cmd ...`)
        if let Some(rest) = text.strip_prefix('!') {
            let cmd = rest.trim().to_string();
            self.clear_input();
            if !cmd.is_empty() {
                self.push(Action::RunShell(cmd));
            }
            return;
        }

        // 3. Normal chat prompt
        if self.is_streaming() {
            self.set_status("A turn is already streaming (Ctrl+X to cancel)");
            return;
        }
        self.clear_input();
        self.new_session_draft = false;
        self.push(Action::SendPrompt(text));
    }

    pub fn execute_slash_command(&mut self, raw: &str, popup_selected: Option<SlashCommandEntry>) {
        let stripped = raw.trim().trim_start_matches('/');
        let mut parts = stripped.splitn(2, char::is_whitespace);
        let typed_cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().map(str::trim).filter(|s| !s.is_empty());

        // If no args were typed and a popup item was highlighted, use the highlighted popup item.
        let resolved_cmd = if arg.is_none() {
            if let Some(sel) = &popup_selected {
                sel.name.clone()
            } else {
                typed_cmd.clone()
            }
        } else {
            typed_cmd.clone()
        };

        // Handle `/harness <name>` entries from CommandPalette (`"harness codex"`)
        if let Some(h_arg) = resolved_cmd.strip_prefix("harness ") {
            if self.is_session_started() {
                self.set_status("Harness is locked for started session; run /new first");
                return;
            }
            let h_arg = h_arg.trim();
            if HARNESSES.contains(&h_arg) {
                self.switch_harness(h_arg, None);
                return;
            }
        }

        match resolved_cmd.as_str() {
            "cd" => {
                if let Some(path_arg) = arg {
                    let target = self.resolve_dir_path(path_arg);
                    self.push(Action::SelectProject(target));
                } else {
                    self.open_overlay(Overlay::DirPicker);
                }
            }
            "sessions" | "s" => {
                self.open_overlay(Overlay::Sessions);
            }
            "new" => {
                if let Some(title) = arg {
                    self.cancel_active_stream();
                    self.current = None;
                    self.current_asd = None;
                    self.cli_session = None;
                    self.new_session_draft = false;
                    self.timeline.clear();
                    self.timeline_sel = 0;
                    self.timeline_scroll = None;
                    self.resume = None;
                    self.pending_permissions.clear();
                    self.permission_sel = 0;
                    self.push(Action::CreateSession { title: Some(title.to_string()) });
                } else {
                    self.start_new_session_draft();
                }
            }
            "models" => {
                self.open_overlay(Overlay::ModelPicker);
            }
            "harness" => {
                if self.is_session_started() {
                    self.set_status("Harness is locked for started session; run /new first");
                    return;
                }
                if let Some(name) = arg {
                    if let Some(&h) = HARNESSES.iter().find(|&&h| h.eq_ignore_ascii_case(name)) {
                        self.switch_harness(h, None);
                    } else {
                        self.set_status(format!("Unknown harness '{name}'. Valid: {}", HARNESSES.join(", ")));
                    }
                } else {
                    let i = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
                    let next = HARNESSES[(i + 1) % HARNESSES.len()];
                    self.switch_harness(next, None);
                }
            }
            "auto-approve" => match arg.map(|s| s.to_lowercase()).as_deref() {
                Some("on" | "true" | "1") => self.set_auto_approve(true),
                Some("off" | "false" | "0") => self.set_auto_approve(false),
                None => self.set_auto_approve(!self.auto_approve),
                Some(other) => self.set_status(format!("Usage: /auto-approve [on|off] (got '{other}')")),
            },
            "board" => self.open_overlay(Overlay::Board),
            "diff" => self.open_overlay(Overlay::Diff),
            "subagents" => self.open_overlay(Overlay::Subagents),
            "routing" => self.open_overlay(Overlay::Routing),
            "terminal" => self.open_overlay(Overlay::Terminal),
            "theme" => {
                self.toggle_theme();
            }
            "sidebar" => {
                self.sidebar_open = !self.sidebar_open;
                if !self.sidebar_open && matches!(self.focus, Focus::Projects | Focus::Sessions) {
                    self.focus = Focus::Input;
                }
                self.set_status(format!("Sidebar {}", if self.sidebar_open { "shown" } else { "hidden" }));
            }
            "clear" => {
                self.timeline.clear();
                self.timeline_sel = 0;
                self.timeline_scroll = None;
                self.set_status("Timeline cleared");
            }
            "help" => self.open_overlay(Overlay::Help),
            "exit" | "quit" => {
                if self.is_streaming() {
                    self.push(Action::CancelTurn);
                }
                self.should_quit = true;
            }
            _ => {
                // Harness-specific `/command` or custom slash prompt → forward to turn stream!
                if self.is_streaming() {
                    self.set_status("A turn is already streaming (Ctrl+X to cancel)");
                    return;
                }
                let prompt = if let Some(a) = arg {
                    format!("/{resolved_cmd} {a}")
                } else {
                    format!("/{resolved_cmd}")
                };
                self.new_session_draft = false;
                self.push(Action::SendPrompt(prompt));
            }
        }
    }

    // --------------------------------------------------------- overlay keys

    fn handle_overlay_key(&mut self, o: Overlay, k: KeyEvent) {
        if o == Overlay::Routing && self.routing_editor.is_some() {
            self.handle_routing_editor_key(k);
            return;
        }
        if k.code == KeyCode::Esc {
            self.overlay = None;
            return;
        }
        match o {
            Overlay::Help => {}
            Overlay::ModelPicker => self.handle_picker_key(k),
            Overlay::Sessions => self.handle_sessions_key(k),
            Overlay::DirPicker => self.handle_dir_picker_key(k),
            Overlay::CommandPalette => self.handle_palette_key(k),
            Overlay::Board => self.handle_board_key(k),
            Overlay::Subagents => self.handle_subagents_key(k),
            Overlay::Diff => self.handle_diff_key(k),
            Overlay::Routing => self.handle_routing_key(k),
            Overlay::Terminal => self.handle_terminal_key(k),
        }
    }

    fn handle_picker_key(&mut self, k: KeyEvent) {
        let n = self.picker_models().len();
        let locked = self.is_session_started();
        if locked {
            self.picker_harness = HARNESSES.iter().position(|h| *h == self.harness).unwrap_or(0);
        }
        match k.code {
            KeyCode::Up => self.picker_sel = self.picker_sel.saturating_sub(1),
            KeyCode::Down => self.picker_sel = (self.picker_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Tab | KeyCode::Right => {
                if !locked {
                    self.picker_harness = (self.picker_harness + 1) % HARNESSES.len();
                    self.picker_sel = 0;
                }
            }
            KeyCode::BackTab | KeyCode::Left => {
                if !locked {
                    self.picker_harness = (self.picker_harness + HARNESSES.len() - 1) % HARNESSES.len();
                    self.picker_sel = 0;
                }
            }
            KeyCode::F(5) => self.push(Action::RefreshModels { force: true }),
            KeyCode::Backspace => {
                self.picker_query.pop();
                self.picker_sel = 0;
            }
            KeyCode::Char(c) => {
                self.picker_query.push(c);
                self.picker_sel = 0;
            }
            KeyCode::Enter => {
                let models = self.picker_models();
                let new_harness = if locked {
                    self.harness.clone()
                } else {
                    HARNESSES[self.picker_harness].to_string()
                };
                let new_model = models.get(self.picker_sel).filter(|_| self.picker_sel > 0).cloned();
                self.overlay = None;
                self.switch_harness(&new_harness, new_model);
            }
            _ => {}
        }
    }

    fn handle_permission_key(&mut self, k: KeyEvent) {
        let Some((turn_id, perm)) = self.pending_permissions.front().cloned() else {
            return;
        };
        let option_count = if perm.options.is_empty() { 2 } else { perm.options.len() };
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.permission_sel = self.permission_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.permission_sel = (self.permission_sel + 1).min(option_count.saturating_sub(1));
            }
            KeyCode::Enter => {
                let (outcome, option_id, is_always) = if !perm.options.is_empty() {
                    let opt = perm
                        .options
                        .get(self.permission_sel)
                        .or_else(|| perm.options.first())
                        .cloned()
                        .unwrap();
                    let always = opt.kind == "allow_always" || opt.option_id == "allow_always";
                    ("selected".to_string(), Some(opt.option_id), always)
                } else if self.permission_sel == 0 {
                    ("selected".to_string(), Some("allow_once".to_string()), false)
                } else {
                    ("cancelled".to_string(), None, false)
                };
                if is_always {
                    self.auto_approve = true;
                }
                self.pending_permissions.pop_front();
                self.permission_sel = 0;
                self.push(Action::RespondPermission {
                    turn_id,
                    request_id: perm.request_id,
                    outcome,
                    option_id,
                });
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let opt_id = perm
                    .options
                    .iter()
                    .find(|o| o.kind == "allow_once" || o.option_id == "allow_once")
                    .or_else(|| {
                        perm.options
                            .iter()
                            .find(|o| o.kind.starts_with("allow") || o.option_id.contains("allow"))
                    })
                    .map(|o| o.option_id.clone())
                    .or_else(|| perm.options.first().map(|o| o.option_id.clone()))
                    .unwrap_or_else(|| "allow_once".to_string());
                self.pending_permissions.pop_front();
                self.permission_sel = 0;
                self.push(Action::RespondPermission {
                    turn_id,
                    request_id: perm.request_id,
                    outcome: "selected".to_string(),
                    option_id: Some(opt_id),
                });
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                let opt_id = perm
                    .options
                    .iter()
                    .find(|o| o.kind == "allow_always" || o.option_id == "allow_always")
                    .or_else(|| {
                        perm.options
                            .iter()
                            .find(|o| o.kind.starts_with("allow") || o.option_id.contains("allow"))
                    })
                    .map(|o| o.option_id.clone())
                    .unwrap_or_else(|| "allow_always".to_string());
                self.auto_approve = true;
                self.pending_permissions.pop_front();
                self.permission_sel = 0;
                self.push(Action::RespondPermission {
                    turn_id,
                    request_id: perm.request_id,
                    outcome: "selected".to_string(),
                    option_id: Some(opt_id),
                });
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let opt = perm
                    .options
                    .iter()
                    .find(|o| o.kind == "reject_once" || o.option_id == "reject_once")
                    .or_else(|| {
                        perm.options
                            .iter()
                            .find(|o| o.kind.starts_with("reject") || o.option_id.contains("reject"))
                    });
                let (outcome, option_id) = match opt {
                    Some(o) => ("selected".to_string(), Some(o.option_id.clone())),
                    None => ("cancelled".to_string(), None),
                };
                self.pending_permissions.pop_front();
                self.permission_sel = 0;
                self.push(Action::RespondPermission {
                    turn_id,
                    request_id: perm.request_id,
                    outcome,
                    option_id,
                });
            }
            _ => {}
        }
    }

    fn handle_dir_picker_key(&mut self, k: KeyEvent) {
        let items = self.dir_picker_items();
        let n = items.len();
        match k.code {
            KeyCode::Up => self.dir_sel = self.dir_sel.saturating_sub(1),
            KeyCode::Down => self.dir_sel = (self.dir_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Tab => {
                if let Some((path, _)) = items.get(self.dir_sel) {
                    self.dir_query = format!("{}/", path.trim_end_matches('/'));
                    self.dir_sel = 0;
                    self.push(Action::ListDirs { prefix: Some(self.dir_query.clone()) });
                }
            }
            KeyCode::Backspace => {
                self.dir_query.pop();
                self.dir_sel = 0;
                let prefix = (!self.dir_query.is_empty()).then(|| self.dir_query.clone());
                self.push(Action::ListDirs { prefix });
            }
            KeyCode::Char(c) => {
                self.dir_query.push(c);
                self.dir_sel = 0;
                self.push(Action::ListDirs { prefix: Some(self.dir_query.clone()) });
            }
            KeyCode::Enter => {
                let target = items
                    .get(self.dir_sel)
                    .map(|(p, _)| p.clone())
                    .or_else(|| (!self.dir_query.trim().is_empty()).then(|| self.resolve_dir_path(&self.dir_query)));
                self.overlay = None;
                if let Some(root) = target {
                    self.push(Action::SelectProject(root));
                }
            }
            _ => {}
        }
    }

    fn handle_palette_key(&mut self, k: KeyEvent) {
        let items = self.palette_items();
        let n = items.len();
        match k.code {
            KeyCode::Up => self.palette_sel = self.palette_sel.saturating_sub(1),
            KeyCode::Down => self.palette_sel = (self.palette_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.palette_sel = 0;
            }
            KeyCode::Char(c) => {
                self.palette_query.push(c);
                self.palette_sel = 0;
            }
            KeyCode::Enter => {
                self.overlay = None;
                if let Some(item) = items.get(self.palette_sel).cloned() {
                    let cmd = format!("/{}", item.name);
                    self.execute_slash_command(&cmd, Some(item));
                }
            }
            _ => {}
        }
    }

    fn handle_board_key(&mut self, k: KeyEvent) {
        match self.board_pane {
            BoardPane::Tasks => {
                let n = self.board_tasks().len();
                if nav(&mut self.board_sel, n, k.code) {
                    return;
                }
            }
            BoardPane::Plan => match k.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.plan_scroll = self.plan_scroll.saturating_sub(1);
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.plan_scroll = self.plan_scroll.saturating_add(1);
                    return;
                }
                KeyCode::PageUp => {
                    self.plan_scroll = self.plan_scroll.saturating_sub(10);
                    return;
                }
                KeyCode::PageDown => {
                    self.plan_scroll = self.plan_scroll.saturating_add(10);
                    return;
                }
                _ => {}
            },
        }
        match k.code {
            KeyCode::Tab => {
                self.board_pane = match self.board_pane {
                    BoardPane::Tasks => BoardPane::Plan,
                    BoardPane::Plan => BoardPane::Tasks,
                }
            }
            KeyCode::Char('s') => self.push(Action::SyncPlan),
            KeyCode::Char('g') => self.push(Action::Dispatch { mode: "parallel_spawn".into() }),
            KeyCode::Char('c') => self.push(Action::Dispatch { mode: "claim_only".into() }),
            KeyCode::Char('b') => self.open_prompt(PromptPurpose::DebateTopic, String::new()),
            KeyCode::Char('r') | KeyCode::F(5) => {
                self.push(Action::RefreshBoard);
                self.push(Action::LoadPlan);
            }
            _ => {}
        }
    }

    fn handle_subagents_key(&mut self, k: KeyEvent) {
        let n = self.subagents.len();
        let before = self.sub_sel;
        if nav(&mut self.sub_sel, n, k.code) {
            if before != self.sub_sel {
                self.logs.clear();
                if let Some(s) = self.subagents.get(self.sub_sel) {
                    let id = s.sub_session_id.clone();
                    self.push(Action::TailLogs(id));
                }
            }
            return;
        }
        if k.code == KeyCode::F(5) {
            self.push(Action::RefreshSubagents);
            return;
        }
        let Some(id) = self.subagents.get(self.sub_sel).map(|s| s.sub_session_id.clone()) else {
            return;
        };
        let act = |a| Action::SubagentAction { sub_session_id: id.clone(), action: a, reason: None };
        match k.code {
            KeyCode::Char('x') => {
                self.confirm = Some((format!("Kill subagent {id}?"), act(SubagentControlActionKind::Kill)))
            }
            KeyCode::Char('r') => self.push(act(SubagentControlActionKind::Retry)),
            KeyCode::Char('f') => self.push(act(SubagentControlActionKind::Fallback)),
            KeyCode::Char('u') => self.push(act(SubagentControlActionKind::Resume)),
            KeyCode::Char('c') => {
                self.open_prompt(PromptPurpose::SubagentCustom { sub_session_id: id }, String::new())
            }
            KeyCode::Enter => self.push(Action::TailLogs(id)),
            _ => {}
        }
    }

    fn handle_diff_key(&mut self, k: KeyEvent) {
        let n = self.board_tasks().len();
        if nav(&mut self.diff_sel, n, k.code) {
            return;
        }
        let task = self.board_tasks().get(self.diff_sel).map(|t| t.id.clone());
        match k.code {
            KeyCode::PageUp => self.diff_scroll = self.diff_scroll.saturating_sub(10),
            KeyCode::PageDown => self.diff_scroll = self.diff_scroll.saturating_add(10),
            KeyCode::F(5) => self.push(Action::RefreshBoard),
            _ => {
                let Some(task_id) = task else { return };
                match k.code {
                    KeyCode::Enter => {
                        self.diff_scroll = 0;
                        self.push(Action::LoadDiff(task_id));
                    }
                    KeyCode::Char('a') => {
                        self.confirm = Some((format!("Approve and merge {task_id}?"), Action::Approve(task_id)))
                    }
                    KeyCode::Char('x') => {
                        self.open_prompt(PromptPurpose::RejectReason { task_id }, String::new())
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_routing_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.routing_scroll = self.routing_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.routing_scroll = self.routing_scroll.saturating_add(1),
            KeyCode::Char('e') => {
                let text = self
                    .routing
                    .as_ref()
                    .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
                    .unwrap_or_else(|| "{}".into());
                self.routing_editor = Some(TextArea::from_text(&text));
            }
            KeyCode::Char('x') => {
                self.confirm = Some(("Reset routing config to defaults?".into(), Action::ResetRouting))
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.push(Action::LoadRouting),
            _ => {}
        }
    }

    fn handle_routing_editor_key(&mut self, k: KeyEvent) {
        let Some(ed) = self.routing_editor.as_mut() else { return };
        if ctrl(&k, 's') {
            let text = ed.text();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(_) => {
                    self.push(Action::SaveRouting(text));
                    self.routing_editor = None;
                }
                Err(e) => self.set_status(format!("Invalid JSON: {e}")),
            }
            return;
        }
        match k.code {
            KeyCode::Esc => {
                self.routing_editor = None;
                self.set_status("Edit cancelled");
            }
            KeyCode::Enter => ed.newline(),
            KeyCode::Backspace => ed.backspace(),
            KeyCode::Left => ed.left(),
            KeyCode::Right => ed.right(),
            KeyCode::Up => ed.up(),
            KeyCode::Down => ed.down(),
            KeyCode::Home => ed.home(),
            KeyCode::End => ed.end(),
            KeyCode::Tab => {
                ed.insert(' ');
                ed.insert(' ');
            }
            KeyCode::Char(c) => ed.insert(c),
            _ => {}
        }
    }

    fn handle_terminal_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('k') | KeyCode::Tab => self.terminal_kind = (self.terminal_kind + 1) % TERMINALS.len(),
            KeyCode::Char('r') | KeyCode::F(5) => {
                if let Some(id) = self.active_session_id() {
                    self.push(Action::LoadResume(id));
                }
            }
            KeyCode::Enter | KeyCode::Char('o') => match self.active_session_id() {
                Some(id) => self.push(Action::LaunchTerminal { session_id: id }),
                None => self.set_status("No ASD session selected"),
            },
            _ => {}
        }
    }
}

pub fn phase_name(p: SessionPhase) -> &'static str {
    match p {
        SessionPhase::Planning => "planning",
        SessionPhase::Running => "running",
        SessionPhase::Paused => "paused",
        SessionPhase::Done => "done",
    }
}

pub fn terminal_name(k: TerminalKind) -> &'static str {
    match k {
        TerminalKind::WindowsTerminal => "Windows Terminal",
        TerminalKind::Powershell => "PowerShell",
        TerminalKind::Cmd => "cmd",
    }
}
