//! The egui desktop application: Antigravity-style Chat + separated Dashboard.
//!
//! All rendering goes through [`NexusApp::ui`], which only needs an
//! `egui::Context`, so the full UI can be exercised headlessly in tests
//! without a window or GPU.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use asd_nexus_client::*;
use egui::{Color32, Frame, Margin, RichText, Rounding, ScrollArea, Stroke, TextEdit};

use crate::backend::{resume_to_spec, Backend, Reply, Request, SavePlanOutcome};
use crate::state::*;
use crate::theme::{self, BadgeStyle, ThemeMode, ThemePalette};
use crate::timeline::{Entry, EntryKind, Timeline, Usage};

// ---------------------------------------------------------------------------
// Sub-states
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    Offline,
    Connecting,
    Connected(String),
    Failed(String),
}

pub struct ChatState {
    pub harness_idx: usize,
    pub picker: ModelPicker,
    pub timeline: Timeline,
    pub composer: String,
    pub turn_seq: Option<u64>,
    pub handle: Option<TurnStreamHandle>,
    pub cancel_requested: bool,
    pub harness_commands: Vec<HarnessCommandItem>,
    pub fs_entries: Vec<FsListEntry>,
    pub last_at_prefix: Option<String>,
    pub popup_selected: usize,
    pub session_filter: String,
    pub new_session_draft: bool,
    pub auto_approve: bool,
    pub pending_permissions: VecDeque<(String, PermissionRequestPayload)>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            harness_idx: 0,
            picker: ModelPicker::default(),
            timeline: Timeline::default(),
            composer: String::new(),
            turn_seq: None,
            handle: None,
            cancel_requested: false,
            harness_commands: Vec::new(),
            fs_entries: Vec::new(),
            last_at_prefix: None,
            popup_selected: 0,
            session_filter: String::new(),
            new_session_draft: true,
            auto_approve: true,
            pending_permissions: VecDeque::new(),
        }
    }
}

impl ChatState {
    pub fn harness(&self) -> &'static str {
        HARNESSES[self.harness_idx.min(HARNESSES.len() - 1)]
    }
    pub fn is_streaming(&self) -> bool {
        self.turn_seq.is_some()
    }
}

#[derive(Default)]
pub struct BoardState {
    pub board: Option<GetBoardResponse>,
    pub selected_task: Option<String>,
    pub dispatch_claim_only: bool,
    pub debate_topic: String,
    pub debate_proposal: String,
    pub last_result: Option<String>,
}

#[derive(Default)]
pub struct DiffState {
    pub task_id: String,
    pub turn: String,
    pub merge_base: bool,
    pub resp: Option<WorktreeDiffResponse>,
    pub approve_summary: String,
    pub reject_reason: String,
    pub last_result: Option<String>,
}

pub struct SubagentsState {
    pub list: Vec<SubagentRecordItem>,
    pub selected: Option<String>,
    pub logs: Vec<String>,
    pub auto_refresh: bool,
    pub last_poll: Option<Instant>,
    pub tail_lines: usize,
    pub fallback_harness: usize,
    pub fallback_model: String,
    pub custom_reason: String,
    pub last_result: Option<String>,
}

impl Default for SubagentsState {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            selected: None,
            logs: Vec::new(),
            auto_refresh: true,
            last_poll: None,
            tail_lines: 200,
            fallback_harness: 0,
            fallback_model: String::new(),
            custom_reason: String::new(),
            last_result: None,
        }
    }
}

pub struct TerminalState {
    pub resume: Option<GetResumeCommandResponse>,
    pub kind: TerminalKind,
    pub last_result: Option<String>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            resume: None,
            kind: if cfg!(windows) { TerminalKind::WindowsTerminal } else { TerminalKind::Powershell },
            last_result: None,
        }
    }
}

/// Window-run options (non-interactive smoke helpers).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub exit_after: Option<Duration>,
    pub screenshot: Option<PathBuf>,
}

/// Actionable item in the `Ctrl+P` / `Ctrl+K` Command Palette.
#[derive(Debug, Clone)]
pub enum PaletteAction {
    SwitchMode(PrimaryMode),
    SwitchDashboardTab(Tab),
    ToggleTheme,
    SwitchHarness(usize),
    SelectModel(usize, Option<String>),
    SwitchProject(String),
    SelectSession(usize),
    SlashCommand(SlashPopupItem),
}

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub category: &'static str,
    pub label: String,
    pub detail: String,
    pub action: PaletteAction,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct NexusApp {
    pub backend: Backend,
    pub conn: ConnState,
    pub health: Option<GetHealthResponse>,
    pub project_root: String,
    pub projects: Vec<ProjectSummaryItem>,
    pub sessions: Vec<DiscoveredSessionItem>,
    pub selected_session: Option<(String, String)>,
    pub active_session_id: Option<String>,
    pub mode: PrimaryMode,
    pub dashboard_tab: Tab,
    pub tab: Tab,
    last_synced_tab: Tab,
    last_synced_mode: PrimaryMode,
    pub theme_mode: ThemeMode,
    applied_theme: Option<ThemeMode>,
    pub sidebar_open: bool,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub chat: ChatState,
    pub board: BoardState,
    pub plan: PlanEditor,
    pub diff: DiffState,
    pub subagents: SubagentsState,
    pub routing: RoutingEditor,
    pub terminal: TerminalState,
    pub status: String,
    pub log: VecDeque<String>,
    pub font_status: String,
    pub run: RunOptions,
    pub frames: u64,
    started: Instant,
    screenshot_requested: bool,
    pub screenshot_saved: Option<PathBuf>,
    repaint_wired: bool,
}

pub fn clean_root(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

fn short_id(s: &str) -> String {
    if s.chars().count() > 14 {
        let tail: String = s.chars().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{tail}")
    } else {
        s.to_string()
    }
}

fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

impl NexusApp {
    pub fn new(project_root: impl AsRef<Path>, run: RunOptions) -> Self {
        Self {
            backend: Backend::new(None),
            conn: ConnState::Offline,
            health: None,
            project_root: clean_root(project_root.as_ref()),
            projects: Vec::new(),
            sessions: Vec::new(),
            selected_session: None,
            active_session_id: None,
            mode: PrimaryMode::Chat,
            dashboard_tab: Tab::Board,
            tab: Tab::Chat,
            last_synced_tab: Tab::Chat,
            last_synced_mode: PrimaryMode::Chat,
            theme_mode: ThemeMode::DarkColor,
            applied_theme: None,
            sidebar_open: true,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            chat: ChatState::default(),
            board: BoardState::default(),
            plan: PlanEditor::default(),
            diff: DiffState::default(),
            subagents: SubagentsState::default(),
            routing: RoutingEditor::default(),
            terminal: TerminalState::default(),
            status: "ready".into(),
            log: VecDeque::new(),
            font_status: String::new(),
            run,
            frames: 0,
            started: Instant::now(),
            screenshot_requested: false,
            screenshot_saved: None,
            repaint_wired: false,
        }
    }

    /// Wire a context (theme + fonts) — called once from eframe's creation context.
    pub fn setup_context(&mut self, ctx: &egui::Context) {
        theme::apply_mode(ctx, self.theme_mode);
        self.applied_theme = Some(self.theme_mode);
        let loaded = crate::fonts::configure_fonts(ctx);
        self.font_status = crate::fonts::describe(loaded.as_deref());
        self.backend.set_repaint(ctx.clone());
        self.repaint_wired = true;
    }

    /// Switch theme at runtime and immediately update `ctx` visuals.
    pub fn set_theme_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.theme_mode = mode;
        theme::apply_mode(ctx, mode);
        self.applied_theme = Some(mode);
        self.set_status(format!("theme: {}", mode.label()));
    }

    /// Toggle between `ThemeMode::DarkColor` and `ThemeMode::Monochrome`.
    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        let next = self.theme_mode.toggle();
        self.set_theme_mode(ctx, next);
    }

    /// Switch primary mode (`Chat` vs `Dashboard`).
    pub fn set_mode(&mut self, mode: PrimaryMode) {
        self.mode = mode;
        match mode {
            PrimaryMode::Chat => {
                self.tab = Tab::Chat;
            }
            PrimaryMode::Dashboard => {
                if !self.dashboard_tab.is_dashboard() {
                    self.dashboard_tab = Tab::Board;
                }
                self.tab = self.dashboard_tab;
            }
        }
        self.last_synced_mode = self.mode;
        self.last_synced_tab = self.tab;
    }

    /// Select one of the 5 Dashboard sub-tabs and switch to `Dashboard` mode.
    pub fn select_dashboard_tab(&mut self, tab: Tab) {
        if tab.is_dashboard() {
            self.dashboard_tab = tab;
            self.tab = tab;
            self.mode = PrimaryMode::Dashboard;
            self.last_synced_mode = self.mode;
            self.last_synced_tab = self.tab;
        } else {
            self.set_mode(PrimaryMode::Chat);
        }
    }

    /// Keep `self.mode`, `self.dashboard_tab`, and `self.tab` synchronized if
    /// external code (such as headless tests) mutated `self.tab` or `self.mode` directly.
    fn sync_mode_and_tab(&mut self) {
        if self.tab != self.last_synced_tab {
            if self.tab == Tab::Chat {
                self.mode = PrimaryMode::Chat;
            } else {
                self.mode = PrimaryMode::Dashboard;
                self.dashboard_tab = self.tab;
            }
        } else if self.mode != self.last_synced_mode {
            match self.mode {
                PrimaryMode::Chat => self.tab = Tab::Chat,
                PrimaryMode::Dashboard => {
                    if !self.dashboard_tab.is_dashboard() {
                        self.dashboard_tab = Tab::Board;
                    }
                    self.tab = self.dashboard_tab;
                }
            }
        }
        self.last_synced_tab = self.tab;
        self.last_synced_mode = self.mode;
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.log.push_back(s.clone());
        while self.log.len() > 200 {
            self.log.pop_front();
        }
        self.status = s;
    }

    fn request(&mut self, req: Request) {
        if let Err(e) = self.backend.submit(req) {
            self.set_status(e);
        }
    }

    fn root_opt(&self) -> Option<String> {
        Some(self.project_root.clone())
    }

    // ---------------------------------------------------------------- actions

    pub fn bootstrap(&mut self) {
        self.request(Request::Health);
        self.request(Request::Projects { force: false });
        self.request(Request::Models { refresh: false });
        self.refresh_harness_commands();
        self.refresh_fs_list(None);
        self.refresh_project_data();
    }

    pub fn refresh_harness_commands(&mut self) {
        self.request(Request::HarnessCommands(GetHarnessCommandsRequest {
            harness: Some(self.chat.harness().to_string()),
            project_root: self.root_opt(),
        }));
    }

    pub fn refresh_fs_list(&mut self, prefix: Option<String>) {
        self.request(Request::FsList(FsListRequest {
            project_root: self.root_opt(),
            mode: Some(FsListMode::Files),
            prefix,
            limit: Some(200),
        }));
    }

    fn refresh_project_data(&mut self) {
        let root = self.project_root.clone();
        self.request(Request::Sessions { project_root: root.clone() });
        self.refresh_board();
        self.load_plan(false);
        self.request(Request::Routing { project_root: root });
        self.refresh_subagents();
    }

    /// Switch the active project (rail selection, `/cd`, or OS folder picker).
    pub fn switch_project(&mut self, root: String) {
        let root = clean_root(Path::new(&root));
        if root == self.project_root && !self.sessions.is_empty() {
            return;
        }
        self.project_root = root.clone();
        self.sessions.clear();
        self.selected_session = None;
        self.active_session_id = None;
        self.chat.new_session_draft = true;
        self.chat.pending_permissions.clear();
        if !self.chat.is_streaming() {
            self.chat.timeline.clear();
        }
        self.chat.fs_entries.clear();
        self.chat.last_at_prefix = None;
        self.board = BoardState::default();
        self.plan = PlanEditor::default();
        self.diff = DiffState::default();
        self.subagents = SubagentsState::default();
        self.routing = RoutingEditor::default();
        self.terminal.resume = None;
        if !self.projects.iter().any(|p| p.project_root == root) {
            self.projects.insert(
                0,
                ProjectSummaryItem {
                    project_root: root.clone(),
                    project_hash: String::new(),
                    session_count: None,
                    last_active_at: None,
                },
            );
        }
        self.set_status(format!("project: {root}"));
        self.request(Request::SelectProject(root));
        self.refresh_harness_commands();
        self.refresh_fs_list(None);
        self.refresh_project_data();
    }

    pub fn pick_folder(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Select ASD project folder");
        if Path::new(&self.project_root).is_dir() {
            dlg = dlg.set_directory(&self.project_root);
        }
        if let Some(dir) = dlg.pick_folder() {
            self.switch_project(clean_root(&dir));
        }
    }

    pub fn is_session_started(&self) -> bool {
        !self.chat.new_session_draft
    }

    /// Enter a fresh New Session draft (unlocks the harness selector without
    /// creating a backend session until the user sends a prompt).
    pub fn start_new_session_draft(&mut self) {
        if self.chat.is_streaming() {
            self.cancel_turn();
        }
        self.chat.new_session_draft = true;
        self.selected_session = None;
        self.active_session_id = None;
        self.chat.timeline.clear();
        self.terminal.resume = None;
        self.chat.pending_permissions.clear();
        self.set_status("new session — choose harness & model, then send message");
    }

    /// Manually switch the active harness from the Chat composer dropdown,
    /// `/harness`, or Command Palette (only allowed while in New Session draft).
    pub fn change_harness_manually(&mut self, new_harness_idx: usize) {
        if self.is_session_started() {
            self.set_status("harness is locked for started session; click + New Chat (/new) first");
            return;
        }
        if new_harness_idx >= HARNESSES.len() || new_harness_idx == self.chat.harness_idx {
            return;
        }
        self.chat.harness_idx = new_harness_idx;
        let h = self.chat.harness().to_string();
        self.chat.picker.on_harness_changed(&h);
        self.selected_session = None;
        self.active_session_id = None;
        self.chat.timeline.clear();
        self.terminal.resume = None;
        self.chat.harness_commands.clear();
        self.refresh_harness_commands();
    }

    /// Manually switch the active harness by name (e.g. `"claude-code"`, `"codex"`).
    pub fn change_harness_by_name(&mut self, harness: &str) -> bool {
        if let Some(idx) = HARNESSES.iter().position(|h| h.eq_ignore_ascii_case(harness.trim())) {
            self.change_harness_manually(idx);
            true
        } else {
            false
        }
    }

    /// Select an existing session from the left sidebar (follows the session's
    /// harness without overwriting or creating a new session, and locks harness).
    pub fn select_session(&mut self, idx: usize) {
        let Some(s) = self.sessions.get(idx).cloned() else { return };
        self.chat.new_session_draft = false;
        self.chat.auto_approve = s.auto_approve.unwrap_or(true);
        self.chat.pending_permissions.clear();
        self.selected_session = Some((s.harness.clone(), s.harness_session_id.clone()));
        self.active_session_id = s.asd_session_id.clone();
        if let Some(i) = HARNESSES.iter().position(|h| *h == s.harness) {
            let harness_changed = self.chat.harness_idx != i;
            self.chat.harness_idx = i;
            self.chat.picker.on_harness_changed(&s.harness);
            if harness_changed {
                self.refresh_harness_commands();
            }
        }
        self.chat.picker.selected = s.model.clone();
        self.terminal.resume = None;
        if !self.chat.is_streaming() {
            self.chat.timeline.clear();
        }
        self.request(Request::Transcript(GetTranscriptRequest {
            harness: s.harness.clone(),
            harness_session_id: s.harness_session_id.clone(),
            project_root: Some(s.project_root.clone()),
            transcript_path: s.transcript_path.clone(),
            max_messages: Some(400),
            before: None,
        }));
        if self.active_session_id.is_some() {
            self.refresh_board();
            self.load_plan(false);
            self.refresh_subagents();
        }
    }

    pub fn selected_session_item(&self) -> Option<&DiscoveredSessionItem> {
        let (h, id) = self.selected_session.as_ref()?;
        self.sessions.iter().find(|s| &s.harness == h && &s.harness_session_id == id)
    }

    pub fn create_session(&mut self) {
        let req = CreateSessionRequest {
            project_root: self.root_opt(),
            harness: Some(self.chat.harness().to_string()),
            model: self.chat.picker.selected.clone(),
            title: None,
            memo: None,
            make_foreground: Some(true),
            auto_approve: Some(self.chat.auto_approve),
        };
        match self.backend.submit(Request::CreateSession(req)) {
            Ok(_) => {
                self.chat.new_session_draft = false;
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Update auto-approve locally and persist to the backend if a session is active.
    pub fn set_auto_approve(&mut self, enabled: bool) {
        self.chat.auto_approve = enabled;
        self.set_status(format!("auto-approve: {}", if enabled { "ON" } else { "OFF" }));
        if let Some(sid) = self.active_session_id.clone() {
            self.request(Request::SetSessionAutoApprove(SetSessionAutoApproveRequest {
                project_root: self.root_opt(),
                session_id: sid,
                auto_approve: enabled,
            }));
        }
    }

    /// Pop the front pending permission request and submit the user's decision.
    pub fn respond_to_permission(&mut self, outcome: &str, option_id: Option<String>) {
        let Some((mut turn_id, perm)) = self.chat.pending_permissions.pop_front() else {
            return;
        };
        if turn_id.is_empty() {
            turn_id = self
                .chat
                .timeline
                .turn_id
                .clone()
                .or_else(|| self.chat.handle.as_ref().and_then(|h| h.turn_id()))
                .unwrap_or_default();
        }
        if let Some(ref oid) = option_id {
            let is_allow_always = perm
                .options
                .iter()
                .any(|o| &o.option_id == oid && o.kind == "allow_always")
                || (perm.options.is_empty() && oid == "allow_always");
            if is_allow_always {
                self.chat.auto_approve = true;
            }
        }
        self.request(Request::SubmitTurnPermission(TurnPermissionDecisionRequest {
            turn_id,
            request_id: perm.request_id,
            outcome: outcome.to_string(),
            option_id,
        }));
    }

    pub fn promote_selected(&mut self) {
        let Some(s) = self.selected_session_item().cloned() else { return };
        self.request(Request::PromoteSession(PromoteSessionRequest {
            project_root: self.project_root.clone(),
            harness: s.harness,
            harness_session_id: s.harness_session_id,
            model: s.model,
            title: None,
            memo: None,
            transcript_path: s.transcript_path,
            make_foreground: Some(true),
            source: Some("desktop".into()),
        }));
    }

    /// Current `/` autocomplete items when the composer is typing a slash command.
    pub fn current_slash_items(&self) -> Vec<SlashPopupItem> {
        match extract_slash_query(&self.chat.composer) {
            Some(q) => build_slash_items(
                q,
                self.chat.harness(),
                &self.chat.harness_commands,
                !self.is_session_started(),
            ),
            None => Vec::new(),
        }
    }

    /// Current `@` file autocomplete entries when the composer has a trailing `@query`.
    pub fn current_at_items(&self) -> Vec<FsListEntry> {
        match extract_at_query(&self.chat.composer) {
            Some((_, q)) => filter_fs_entries(&self.chat.fs_entries, q, 12).into_iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Execute or insert a slash popup item by index.
    pub fn accept_slash_item(&mut self, idx: usize, ctx: Option<&egui::Context>) -> bool {
        let items = self.current_slash_items();
        let Some(item) = items.get(idx).cloned() else { return false };
        self.apply_slash_popup_item(&item, ctx);
        true
    }

    pub fn apply_slash_popup_item(&mut self, item: &SlashPopupItem, ctx: Option<&egui::Context>) {
        self.chat.popup_selected = 0;
        if item.is_nexus {
            match item.name.as_str() {
                "new" => {
                    self.chat.composer.clear();
                    self.start_new_session_draft();
                }
                "models" => {
                    self.chat.composer.clear();
                    self.set_status("refreshing models…");
                    self.request(Request::Models { refresh: true });
                }
                "dashboard" => {
                    self.chat.composer.clear();
                    self.set_mode(PrimaryMode::Dashboard);
                }
                "theme" => {
                    self.chat.composer.clear();
                    if let Some(c) = ctx {
                        self.toggle_theme(c);
                    } else {
                        self.theme_mode = self.theme_mode.toggle();
                        self.applied_theme = None;
                    }
                }
                "clear" => {
                    self.chat.composer.clear();
                    self.chat.timeline.clear();
                    self.set_status("timeline cleared");
                }
                "sessions" => {
                    self.chat.composer.clear();
                    self.sidebar_open = true;
                    self.request(Request::Sessions { project_root: self.project_root.clone() });
                }
                "cd" => {
                    self.chat.composer = "/cd ".to_string();
                }
                "harness" => {
                    if self.is_session_started() {
                        self.chat.composer.clear();
                        self.set_status("harness is locked for started session; click + New Chat (/new) first");
                    } else {
                        self.chat.composer = "/harness ".to_string();
                    }
                }
                "auto-approve" => {
                    self.chat.composer.clear();
                    self.set_auto_approve(!self.chat.auto_approve);
                }
                _ => {
                    self.chat.composer = format!("/{} ", item.name);
                }
            }
        } else {
            self.chat.composer = format!("/{} ", item.name);
        }
    }

    /// Accept a `@` file autocomplete item by index, inserting `@<path> ` into the composer.
    pub fn accept_at_item(&mut self, idx: usize) -> bool {
        let items = self.current_at_items();
        let Some(entry) = items.get(idx) else { return false };
        self.chat.composer = replace_at_token(&self.chat.composer, &entry.path);
        self.chat.popup_selected = 0;
        true
    }

    /// Try to handle built-in `[nexus]` slash commands when the user submits the composer.
    /// Returns `true` if handled locally as a Nexus command.
    fn try_handle_nexus_slash_command(&mut self, prompt: &str, ctx: Option<&egui::Context>) -> bool {
        let trimmed = prompt.trim();
        let Some(rest) = trimmed.strip_prefix('/') else { return false };
        let (cmd, arg) = match rest.split_once(char::is_whitespace) {
            Some((c, a)) => (c.trim(), a.trim()),
            None => (rest.trim(), ""),
        };
        match cmd {
            "new" if arg.is_empty() => {
                self.chat.composer.clear();
                self.start_new_session_draft();
                true
            }
            "models" if arg.is_empty() => {
                self.chat.composer.clear();
                self.request(Request::Models { refresh: true });
                true
            }
            "dashboard" if arg.is_empty() => {
                self.chat.composer.clear();
                self.set_mode(PrimaryMode::Dashboard);
                true
            }
            "theme" if arg.is_empty() => {
                self.chat.composer.clear();
                if let Some(c) = ctx {
                    self.toggle_theme(c);
                } else {
                    self.theme_mode = self.theme_mode.toggle();
                    self.applied_theme = None;
                }
                true
            }
            "clear" if arg.is_empty() => {
                self.chat.composer.clear();
                self.chat.timeline.clear();
                self.set_status("timeline cleared");
                true
            }
            "sessions" if arg.is_empty() => {
                self.chat.composer.clear();
                self.sidebar_open = true;
                self.request(Request::Sessions { project_root: self.project_root.clone() });
                true
            }
            "cd" => {
                self.chat.composer.clear();
                if arg.is_empty() {
                    self.palette_open = true;
                    self.palette_query = "project:".into();
                } else {
                    let target = if Path::new(arg).is_absolute() {
                        PathBuf::from(arg)
                    } else {
                        Path::new(&self.project_root).join(arg)
                    };
                    self.switch_project(clean_root(&target));
                }
                true
            }
            "harness" => {
                self.chat.composer.clear();
                if self.is_session_started() {
                    self.set_status("harness is locked for started session; click + New Chat (/new) first");
                } else if arg.is_empty() {
                    let next = (self.chat.harness_idx + 1) % HARNESSES.len();
                    self.change_harness_manually(next);
                } else if !self.change_harness_by_name(arg) {
                    self.set_status(format!("unknown harness: {arg}"));
                }
                true
            }
            "auto-approve" => {
                self.chat.composer.clear();
                let next = match arg.to_ascii_lowercase().as_str() {
                    "" | "toggle" => !self.chat.auto_approve,
                    "on" | "true" | "1" | "enable" => true,
                    "off" | "false" | "0" | "disable" => false,
                    other => {
                        self.set_status(format!("unknown auto-approve argument: {other} (use on/off)"));
                        return true;
                    }
                };
                self.set_auto_approve(next);
                true
            }
            _ => false,
        }
    }

    pub fn send_prompt(&mut self) {
        let prompt = self.chat.composer.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        if self.try_handle_nexus_slash_command(&prompt, None) {
            return;
        }
        if self.chat.is_streaming() {
            return;
        }
        let session_id = if self.chat.new_session_draft {
            None
        } else if let Some(ref sid) = self.active_session_id {
            Some(sid.clone())
        } else if let Some((_, ref h_sid)) = self.selected_session {
            Some(h_sid.clone())
        } else {
            None
        };
        let model = if self.is_session_started() && self.chat.picker.selected.is_none() {
            Some("default".into())
        } else {
            self.chat.picker.selected.clone()
        };
        let req = TurnStreamRequest {
            project_root: self.root_opt(),
            session_id,
            harness: self.chat.harness().to_string(),
            model,
            prompt: prompt.clone(),
            source: Some("desktop".into()),
            streaming: Some(true),
        };
        let auto_approve = if self.chat.new_session_draft || req.session_id.is_none() {
            Some(self.chat.auto_approve)
        } else {
            None
        };
        match self.backend.submit(Request::StartTurn { req, auto_approve }) {
            Ok(seq) => {
                self.chat.new_session_draft = false;
                self.chat.timeline.begin_turn(&prompt);
                self.chat.composer.clear();
                self.chat.turn_seq = Some(seq);
                self.chat.handle = None;
                self.chat.cancel_requested = false;
                self.set_status(format!("streaming turn on {}", self.chat.harness()));
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Cancel the running turn via `TurnStreamHandle::cancel`.
    pub fn cancel_turn(&mut self) {
        if !self.chat.is_streaming() {
            return;
        }
        match self.chat.handle.take() {
            Some(h) => {
                self.request(Request::CancelTurn(h));
                self.chat.turn_seq = None;
            }
            // Handle not yet returned: keep the seq so the arriving handle is
            // cancelled immediately; stream output is ignored meanwhile.
            None => self.chat.cancel_requested = true,
        }
        self.chat.pending_permissions.clear();
        self.chat.timeline.end_turn(Some("turn cancelled"));
        self.set_status("cancel requested");
    }

    fn refresh_board(&mut self) {
        self.request(Request::Board(GetBoardRequest {
            project_root: self.root_opt(),
            session_id: self.active_session_id.clone(),
        }));
    }

    fn load_plan(&mut self, force: bool) {
        if force {
            self.plan.loaded = false;
        }
        self.request(Request::Plan { project_root: self.root_opt(), session_id: self.active_session_id.clone() });
    }

    pub fn save_plan(&mut self) {
        let req = self.plan.save_request(self.root_opt(), self.active_session_id.clone());
        let (sha, mtime) = (self.plan.sha256.clone(), self.plan.mtime_ms);
        self.request(Request::SavePlan { req, expected_sha256: sha, expected_mtime_ms: mtime });
    }

    fn session_for_actions(&self) -> Option<String> {
        self.active_session_id
            .clone()
            .or_else(|| self.board.board.as_ref().and_then(|b| b.session_id.clone()))
            .or_else(|| self.plan.session_id.clone())
    }

    fn refresh_subagents(&mut self) {
        self.subagents.last_poll = Some(Instant::now());
        self.request(Request::Subagents(GetSubagentsRequest {
            project_root: self.root_opt(),
            session_id: self.active_session_id.clone(),
            task_id: None,
        }));
        if let Some(id) = self.subagents.selected.clone() {
            self.request(Request::SubagentLogs(SubagentLogTailRequest {
                project_root: self.root_opt(),
                sub_session_id: id,
                tail_lines: Some(self.subagents.tail_lines),
            }));
        }
    }

    fn subagent_action(&mut self, action: SubagentControlActionKind) {
        let Some(id) = self.subagents.selected.clone() else { return };
        let rec = self.subagents.list.iter().find(|s| s.sub_session_id == id).cloned();
        let session_id = rec
            .as_ref()
            .map(|r| r.parent_session_id.clone())
            .or_else(|| self.session_for_actions())
            .unwrap_or_default();
        let (harness, model) = if action == SubagentControlActionKind::Fallback {
            let m = self.subagents.fallback_model.trim();
            (Some(HARNESSES[self.subagents.fallback_harness].to_string()), (!m.is_empty()).then(|| m.to_string()))
        } else {
            (None, None)
        };
        let reason = (!self.subagents.custom_reason.trim().is_empty()).then(|| self.subagents.custom_reason.trim().to_string());
        self.request(Request::SubagentAction(SubagentActionRequest {
            project_root: self.root_opt(),
            session_id,
            sub_session_id: id,
            action,
            harness,
            model,
            reason,
        }));
    }

    pub fn load_diff(&mut self) {
        let task = self.diff.task_id.trim().to_string();
        self.request(Request::Diff(WorktreeDiffRequest {
            project_root: self.root_opt(),
            task_id: (!task.is_empty()).then_some(task),
            turn: self.diff.turn.trim().parse().ok(),
            wt_path: None,
            base: Some(if self.diff.merge_base { "merge-base" } else { "HEAD" }.into()),
        }));
    }

    fn fetch_resume(&mut self) {
        let Some(sid) = self.session_for_actions() else {
            self.set_status("resume: no ASD session selected (promote a CLI session first)");
            return;
        };
        self.request(Request::ResumeCommand { session_id: sid, project_root: self.root_opt() });
    }

    /// Copy the interactive resume command to the OS clipboard (`ctx.copy_text`).
    pub fn copy_resume(&mut self, ctx: &egui::Context) -> bool {
        let Some(cli) = self.terminal.resume.as_ref().map(|r| r.cli_string.clone()) else {
            return false;
        };
        ctx.copy_text(cli);
        self.set_status("resume command copied to clipboard");
        true
    }

    // ---------------------------------------------------------------- replies

    pub fn handle_reply(&mut self, r: Reply) {
        match r {
            Reply::Connected(Ok((url, token))) => {
                self.backend.set_client(Some(NexusClient::new(url.clone(), token)));
                self.conn = ConnState::Connected(url.clone());
                self.set_status(format!("connected: {url}"));
                self.bootstrap();
            }
            Reply::Connected(Err(e)) => {
                self.conn = ConnState::Failed(e.clone());
                self.set_status(format!("sidecar failed: {e}"));
            }
            Reply::Health(r) => match r {
                Ok(h) => {
                    self.set_status(format!("sidecar v{} (pid {})", h.version, h.pid));
                    self.health = Some(h);
                }
                Err(e) => self.set_status(format!("health: {e}")),
            },
            Reply::Projects(r) => match r {
                Ok(p) => {
                    let mut list = p.projects;
                    if !list.iter().any(|x| x.project_root == self.project_root) {
                        list.insert(
                            0,
                            ProjectSummaryItem {
                                project_root: self.project_root.clone(),
                                project_hash: String::new(),
                                session_count: None,
                                last_active_at: None,
                            },
                        );
                    }
                    self.projects = list;
                }
                Err(e) => self.set_status(format!("projects: {e}")),
            },
            Reply::SelectProject(r) => match r {
                Ok(p) if p.success => self.set_status(format!("selected {}", p.project_root)),
                Ok(p) => self.set_status(format!("select project: {}", p.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("select project: {e}")),
            },
            Reply::Sessions(r) => match r {
                Ok(s) => {
                    self.sessions = s.sessions;
                    if self.active_session_id.is_none()
                        && !self.chat.new_session_draft
                        && self.selected_session.is_none()
                    {
                        self.active_session_id = s.active_session_id;
                    }
                }
                Err(e) => self.set_status(format!("sessions: {e}")),
            },
            Reply::CreateSession(r) => match r {
                Ok(c) if c.success => {
                    self.chat.new_session_draft = false;
                    self.chat.auto_approve = c.auto_approve.unwrap_or(self.chat.auto_approve);
                    self.active_session_id = Some(c.session_id.clone());
                    self.selected_session = None;
                    if !self.chat.is_streaming() {
                        self.chat.timeline.clear();
                    }
                    self.set_status(format!("created session {}", c.session_id));
                    self.request(Request::Sessions { project_root: self.project_root.clone() });
                    self.refresh_board();
                }
                Ok(c) => self.set_status(format!("create session: {}", c.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("create session: {e}")),
            },
            Reply::PromoteSession(r) => match r {
                Ok(p) if p.success => {
                    self.active_session_id = Some(p.session_id.clone());
                    self.set_status(format!("promoted → {}", p.session_id));
                    self.request(Request::Sessions { project_root: self.project_root.clone() });
                }
                Ok(p) => self.set_status(format!("promote: {}", p.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("promote: {e}")),
            },
            Reply::Transcript(r) => match r {
                Ok(t) => {
                    if !self.chat.is_streaming() {
                        self.chat.timeline.load_transcript(&t.messages);
                    }
                    self.set_status(format!("transcript: {} messages", t.messages.len()));
                }
                Err(e) => self.set_status(format!("transcript: {e}")),
            },
            Reply::Models(r) => match r {
                Ok(m) => {
                    self.chat.picker.set_models(m.models);
                    let h = self.chat.harness().to_string();
                    self.chat.picker.on_harness_changed(&h);
                    let count = self.chat.picker.models.get(&h).map(Vec::len).unwrap_or(0);
                    self.set_status(format!("models: {count} for {h}"));
                }
                Err(e) => self.set_status(format!("models: {e}")),
            },
            Reply::HarnessCommands(r) => match r {
                Ok(resp) => {
                    self.chat.harness_commands = resp.commands;
                }
                Err(e) => self.set_status(format!("harness commands: {e}")),
            },
            Reply::FsList(r) => match r {
                Ok(resp) => {
                    self.chat.fs_entries = resp.entries;
                }
                Err(e) => self.set_status(format!("fs list: {e}")),
            },
            Reply::Board(r) => match r {
                Ok(b) => self.board.board = Some(b),
                Err(e) => self.set_status(format!("board: {e}")),
            },
            Reply::Plan(r) => match r {
                Ok(p) => {
                    let force = !self.plan.loaded;
                    match self.plan.apply_loaded(p, force) {
                        PlanLoadOutcome::Conflict => self.set_status("PLAN.md conflict: changed on disk"),
                        PlanLoadOutcome::Loaded => self.set_status("PLAN.md loaded"),
                        PlanLoadOutcome::Unchanged => {}
                    }
                }
                Err(e) => self.set_status(format!("plan: {e}")),
            },
            Reply::SavePlan(r) => match r {
                Ok(SavePlanOutcome::Saved(s)) => {
                    self.plan.apply_save_result(&s);
                    if s.success {
                        self.set_status(format!("PLAN.md saved ({} tasks updated)", s.tasks_updated.unwrap_or(0)));
                        self.refresh_board();
                    } else if s.conflict == Some(true) {
                        self.set_status("PLAN.md save rejected: conflict");
                    } else {
                        self.set_status(format!("save plan: {}", s.error.unwrap_or_default()));
                    }
                }
                Ok(SavePlanOutcome::Drift(fresh)) => {
                    self.plan.preflight(&fresh);
                    self.set_status("PLAN.md save blocked: file changed on disk");
                }
                Err(e) => self.set_status(format!("save plan: {e}")),
            },
            Reply::SyncPlan(r) => match r {
                Ok(s) if s.success => {
                    self.set_status(format!("PLAN synced ({} tasks)", s.tasks_updated.unwrap_or(0)));
                    self.refresh_board();
                }
                Ok(s) => self.set_status(format!("sync: {}", s.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("sync: {e}")),
            },
            Reply::Dispatch(r) => {
                let msg = match r {
                    Ok(d) if d.success => format!(
                        "dispatched: {}",
                        d.dispatched.unwrap_or_default().join(", ")
                    ),
                    Ok(d) => format!("dispatch: {}", d.error.or(d.message).unwrap_or_default()),
                    Err(e) => format!("dispatch: {e}"),
                };
                self.board.last_result = Some(msg.clone());
                self.set_status(msg);
                self.refresh_board();
            }
            Reply::Debate(r) => {
                let msg = match r {
                    Ok(d) if d.success => format!(
                        "debate verdict: {}",
                        d.verdict.or(d.message).unwrap_or_else(|| "(none)".into())
                    ),
                    Ok(d) => format!("debate: {}", d.error.unwrap_or_default()),
                    Err(e) => format!("debate: {e}"),
                };
                self.board.last_result = Some(msg.clone());
                self.set_status(msg);
            }
            Reply::Diff(r) => match r {
                Ok(d) => {
                    if let Some(e) = &d.error {
                        self.set_status(format!("diff: {e}"));
                    } else {
                        self.set_status(format!("diff: {} files", d.files.len()));
                    }
                    self.diff.resp = Some(d);
                }
                Err(e) => self.set_status(format!("diff: {e}")),
            },
            Reply::Approve(r) => {
                let msg = match r {
                    Ok(a) if a.success => format!("approved (merged: {})", a.merged.unwrap_or(false)),
                    Ok(a) => format!("approve: {}", a.error.unwrap_or_default()),
                    Err(e) => format!("approve: {e}"),
                };
                self.diff.last_result = Some(msg.clone());
                self.set_status(msg);
                self.refresh_board();
            }
            Reply::Reject(r) => {
                let msg = match r {
                    Ok(a) if a.success => "rejected".to_string(),
                    Ok(a) => format!("reject: {}", a.error.unwrap_or_default()),
                    Err(e) => format!("reject: {e}"),
                };
                self.diff.last_result = Some(msg.clone());
                self.set_status(msg);
                self.refresh_board();
            }
            Reply::Subagents(r) => match r {
                Ok(s) => self.subagents.list = s.subagents,
                Err(e) => self.set_status(format!("subagents: {e}")),
            },
            Reply::SubagentLogs(r) => match r {
                Ok(l) => {
                    if Some(&l.sub_session_id) == self.subagents.selected.as_ref() {
                        self.subagents.logs = l.lines;
                    }
                }
                Err(e) => self.set_status(format!("logs: {e}")),
            },
            Reply::SubagentAction(r) => {
                let msg = match r {
                    Ok(a) if a.success => format!("subagent action ok: {}", a.message.unwrap_or_default()),
                    Ok(a) => format!("subagent action: {}", a.error.or(a.message).unwrap_or_default()),
                    Err(e) => format!("subagent action: {e}"),
                };
                self.subagents.last_result = Some(msg.clone());
                self.set_status(msg);
                self.refresh_subagents();
            }
            Reply::Routing(r) => match r {
                Ok(g) => self.routing.load(g.routing),
                Err(e) => self.set_status(format!("routing: {e}")),
            },
            Reply::SaveRouting(r) => match r {
                Ok(s) if s.success => {
                    self.set_status("routing saved");
                    self.request(Request::Routing { project_root: self.project_root.clone() });
                }
                Ok(s) => self.set_status(format!("routing: {}", s.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("routing: {e}")),
            },
            Reply::ResumeCommand(r) => match r {
                Ok(c) if c.success => {
                    self.set_status(format!("resume: {}", c.cli_string));
                    self.terminal.resume = Some(c);
                }
                Ok(c) => self.set_status(format!("resume: {}", c.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("resume: {e}")),
            },
            Reply::Terminal(r) => {
                let msg = match r {
                    Ok(m) => m,
                    Err(e) => format!("terminal: {e}"),
                };
                self.terminal.last_result = Some(msg.clone());
                self.set_status(msg);
            }
            Reply::TurnStarted { seq, handle } => {
                if self.chat.turn_seq != Some(seq) {
                    return;
                }
                match handle {
                    Ok(h) => {
                        if self.chat.cancel_requested {
                            self.chat.cancel_requested = false;
                            self.chat.turn_seq = None;
                            self.request(Request::CancelTurn(h));
                        } else {
                            self.chat.handle = Some(h);
                        }
                    }
                    Err(e) => {
                        self.chat.timeline.apply(&StreamMessage::Error(e.clone()));
                        self.chat.timeline.end_turn(None);
                        self.chat.turn_seq = None;
                        self.chat.cancel_requested = false;
                        self.chat.pending_permissions.clear();
                        self.set_status(format!("turn failed: {e}"));
                    }
                }
            }
            Reply::Stream { seq, msg } => {
                if self.chat.turn_seq != Some(seq) || self.chat.cancel_requested {
                    return;
                }
                if let StreamMessage::Event(ref evt) = msg {
                    if self.active_session_id.is_none() {
                        let asd_sid = evt
                            .raw
                            .as_ref()
                            .and_then(|v| v.get("asdSessionId"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .or_else(|| {
                                evt.session_id
                                    .as_deref()
                                    .filter(|id| id.starts_with("asd-"))
                                    .map(str::to_string)
                            });
                        if let Some(sid) = asd_sid {
                            self.active_session_id = Some(sid);
                        }
                    }
                    if evt.kind == OrchestratorEventKind::PermissionRequest {
                        if let Some(perm) = evt.permission_request.clone() {
                            let tid = evt
                                .turn_id
                                .clone()
                                .or_else(|| self.chat.timeline.turn_id.clone())
                                .or_else(|| self.chat.handle.as_ref().and_then(|h| h.turn_id()))
                                .unwrap_or_default();
                            self.chat.pending_permissions.push_back((tid, perm));
                        }
                    }
                }
                self.chat.timeline.apply(&msg);
                if !self.chat.timeline.streaming {
                    self.chat.turn_seq = None;
                    self.chat.handle = None;
                    self.chat.pending_permissions.clear();
                    self.set_status(format!("turn done · {}", self.chat.timeline.turn_usage.readout()));
                    self.request(Request::Sessions { project_root: self.project_root.clone() });
                }
            }
            Reply::TurnCancelled(r) => match r {
                Ok(c) => self.set_status(format!(
                    "turn cancel: {}",
                    c.message.or(c.error).unwrap_or_else(|| c.success.to_string())
                )),
                Err(e) => self.set_status(format!("turn cancel: {e}")),
            },
            Reply::SetSessionAutoApprove(r) => match r {
                Ok(resp) if resp.success => {
                    if self.active_session_id.as_deref() == Some(resp.session_id.as_str()) {
                        self.chat.auto_approve = resp.auto_approve;
                    }
                    self.set_status(format!(
                        "auto-approve: {} ({})",
                        if resp.auto_approve { "ON" } else { "OFF" },
                        short_id(&resp.session_id)
                    ));
                }
                Ok(resp) => self.set_status(format!("auto-approve: {}", resp.error.unwrap_or_default())),
                Err(e) => self.set_status(format!("auto-approve: {e}")),
            },
            Reply::SubmitTurnPermission(r) => match r {
                Ok(resp) if resp.success => self.set_status("permission decision submitted"),
                Ok(resp) => self.set_status(format!(
                    "permission decision: {}",
                    resp.error.unwrap_or_else(|| "rejected".into())
                )),
                Err(e) => self.set_status(format!("permission decision: {e}")),
            },
        }
    }

    // ---------------------------------------------------------------- frame

    /// Render one frame. Usable headlessly with `egui::Context::run`.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.repaint_wired {
            self.backend.set_repaint(ctx.clone());
            self.repaint_wired = true;
        }
        if self.applied_theme != Some(self.theme_mode) {
            theme::apply_mode(ctx, self.theme_mode);
            self.applied_theme = Some(self.theme_mode);
        }
        self.sync_mode_and_tab();
        self.frames += 1;
        for r in self.backend.poll() {
            self.handle_reply(r);
        }
        self.shortcuts(ctx);
        self.sync_mode_and_tab();

        let p = theme::palette(self.theme_mode);

        // Live subagent polling while the Subagents dashboard tab is open.
        if self.mode == PrimaryMode::Dashboard
            && self.tab == Tab::Subagents
            && self.subagents.auto_refresh
            && self.backend.is_connected()
        {
            let due = self.subagents.last_poll.map(|t| t.elapsed() > Duration::from_millis(1500)).unwrap_or(true);
            if due && self.backend.in_flight < 4 {
                self.refresh_subagents();
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        egui::TopBottomPanel::top("top").frame(panel_frame(p.bg, p.border)).show(ctx, |ui| self.top_bar(ui));
        if self.mode == PrimaryMode::Dashboard {
            egui::TopBottomPanel::top("dashboard_subtabs")
                .frame(panel_frame(p.panel, p.border))
                .show(ctx, |ui| self.dashboard_subtab_bar(ui));
        }
        egui::TopBottomPanel::bottom("status").frame(panel_frame(p.bg, p.border)).show(ctx, |ui| self.status_bar(ui));

        if self.sidebar_open {
            egui::SidePanel::left("rail")
                .resizable(true)
                .default_width(280.0)
                .min_width(220.0)
                .frame(panel_frame(p.panel, p.border))
                .show(ctx, |ui| self.rail(ui));
        }

        egui::CentralPanel::default().frame(panel_frame(p.bg, p.border)).show(ctx, |ui| match self.mode {
            PrimaryMode::Chat => self.chat_tab(ui),
            PrimaryMode::Dashboard => match self.dashboard_tab {
                Tab::Board | Tab::Chat => self.board_tab(ui),
                Tab::Diff => self.diff_tab(ui),
                Tab::Subagents => self.subagents_tab(ui),
                Tab::Routing => self.routing_tab(ui),
                Tab::Terminal => self.terminal_tab(ui),
            },
        });

        if self.palette_open {
            self.command_palette_modal(ctx);
        }
        if !self.chat.pending_permissions.is_empty() {
            self.permission_request_modal(ctx);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let mut mode_switch = None;
        let mut subtab_switch = None;
        let mut toggle_palette = false;
        let mut close_palette = false;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num1) {
                mode_switch = Some(PrimaryMode::Chat);
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num2) {
                mode_switch = Some(PrimaryMode::Dashboard);
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num3) {
                subtab_switch = Some(Tab::Diff);
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num4) {
                subtab_switch = Some(Tab::Subagents);
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num5) {
                subtab_switch = Some(Tab::Routing);
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num6) {
                subtab_switch = Some(Tab::Terminal);
            }
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::P)
                || i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)
            {
                toggle_palette = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                close_palette = true;
            }
        });
        if let Some(m) = mode_switch {
            self.set_mode(m);
        }
        if let Some(t) = subtab_switch {
            self.select_dashboard_tab(t);
        }
        if toggle_palette {
            self.palette_open = !self.palette_open;
            if self.palette_open {
                self.palette_query.clear();
                self.palette_selected = 0;
            }
        } else if close_palette && self.palette_open {
            self.palette_open = false;
        }
    }

    // ---------------------------------------------------------------- top header & subtab bar

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            let toggle_icon = if self.sidebar_open { "«" } else { "☰" };
            if ui
                .button(RichText::new(toggle_icon).color(p.text_hi))
                .on_hover_text("Toggle Workspaces & Sessions sidebar")
                .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }

            ui.label(RichText::new("ASD NEXUS").strong().size(17.0).color(p.white));
            let (txt, st) = match &self.conn {
                ConnState::Connected(_) => ("CONNECTED", theme::status_badge_for(tm, "connected")),
                ConnState::Connecting => ("CONNECTING", theme::status_badge_for(tm, "connecting")),
                ConnState::Failed(_) => ("OFFLINE", theme::status_badge_for(tm, "offline")),
                ConnState::Offline => ("OFFLINE", theme::status_badge_for(tm, "pending")),
            };
            badge(ui, txt, st);
            ui.separator();

            // Primary segmented mode switch: [ Chat ] | [ Dashboard ] (Ctrl+1 / Ctrl+2)
            Frame::none()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .inner_margin(Margin::symmetric(4.0, 2.0))
                .rounding(Rounding::same(p.pill_rounding))
                .show(ui, |ui| {
                    for (m, shortcut_hint) in [(PrimaryMode::Chat, "Ctrl+1"), (PrimaryMode::Dashboard, "Ctrl+2")] {
                        let sel = self.mode == m;
                        let text = if sel {
                            RichText::new(format!(" {} ", m.label())).strong().color(p.white)
                        } else {
                            RichText::new(format!(" {} ", m.label())).color(p.dim)
                        };
                        if ui
                            .selectable_label(sel, text)
                            .on_hover_text(format!("Switch to {} ({shortcut_hint})", m.label()))
                            .clicked()
                        {
                            self.set_mode(m);
                        }
                    }
                });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.backend.in_flight > 0 || self.chat.is_streaming() {
                    ui.spinner();
                }

                // [ Dark | Mono ] theme toggle button
                let theme_label = match self.theme_mode {
                    ThemeMode::DarkColor => "Dark | Mono",
                    ThemeMode::Monochrome => "Mono | Dark",
                };
                if ui
                    .button(RichText::new(theme_label).small().strong().color(p.text_hi))
                    .on_hover_text("Toggle Antigravity Dark / Monochrome theme (/theme)")
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.toggle_theme(&ctx);
                }

                // Ctrl+P Command Palette button
                if ui
                    .button(RichText::new("Ctrl+P").small().monospace().color(p.text_hi))
                    .on_hover_text("Open Command Palette (Ctrl+P / Ctrl+K)")
                    .clicked()
                {
                    self.palette_open = !self.palette_open;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                }

                // Current workspace folder badge (click to switch folder/project)
                let ws_name = basename(&self.project_root);
                if ui
                    .button(RichText::new(format!("📁 {ws_name}")).small().color(p.text_hi))
                    .on_hover_text(format!("{}\nClick to switch project", self.project_root))
                    .clicked()
                {
                    self.palette_open = true;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                }
            });
        });
    }

    fn dashboard_subtab_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        ui.horizontal(|ui| {
            ui.label(RichText::new("DASHBOARD").small().strong().color(p.muted));
            ui.separator();
            for t in Tab::DASHBOARD {
                let sel = self.dashboard_tab == t;
                let text = if sel {
                    RichText::new(t.label()).strong().color(p.white)
                } else {
                    RichText::new(t.label()).color(p.dim)
                };
                if ui.selectable_label(sel, text).clicked() {
                    self.select_dashboard_tab(t);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).small().color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let ConnState::Connected(u) = &self.conn {
                    ui.label(RichText::new(u).small().monospace().color(p.muted));
                }
                if !self.font_status.is_empty() {
                    ui.label(RichText::new(&self.font_status).small().color(p.muted));
                }
                ui.label(RichText::new(format!("theme: {}", self.theme_mode.label())).small().color(p.muted));
            });
        });
    }

    // ---------------------------------------------------------------- left sidebar (Workspaces & Sessions)

    fn rail(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;

        ui.horizontal(|ui| {
            section_title(ui, "WORKSPACES", p);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("«").on_hover_text("Collapse sidebar").clicked() {
                    self.sidebar_open = false;
                }
                if ui.small_button("↻").on_hover_text("Refresh projects and sessions").clicked() {
                    self.request(Request::Projects { force: true });
                    self.request(Request::Sessions { project_root: self.project_root.clone() });
                }
                if ui.small_button("Open…").on_hover_text("Open Folder… (native OS folder picker)").clicked() {
                    self.pick_folder();
                }
            });
        });

        let current = basename(&self.project_root);
        let mut switch_to = None;
        let combo = egui::ComboBox::from_id_salt("project")
            .selected_text(RichText::new(current).color(p.text_hi))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("project-list")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for proj in &self.projects {
                            let sel = proj.project_root == self.project_root;
                            let label = match proj.session_count {
                                Some(n) => format!("{}  ·  {n}", basename(&proj.project_root)),
                                None => basename(&proj.project_root),
                            };
                            if ui.selectable_label(sel, label).on_hover_text(&proj.project_root).clicked() && !sel {
                                switch_to = Some(proj.project_root.clone());
                            }
                        }
                    });
            });
        combo.response.on_hover_text(&self.project_root);
        if let Some(r) = switch_to {
            self.switch_project(r);
        }

        ui.add_space(6.0);
        if ui
            .add_sized(
                [ui.available_width(), 30.0],
                egui::Button::new(RichText::new("+ New Chat").strong().color(p.white)).fill(p.accent_fill),
            )
            .on_hover_text("Start a new session in the active harness (/new)")
            .clicked()
        {
            self.start_new_session_draft();
            self.set_mode(PrimaryMode::Chat);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            section_title(ui, "SESSIONS", p);
            if let Some(id) = &self.active_session_id {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(short_id(id)).small().monospace().color(p.muted));
                });
            }
        });

        ui.add(
            TextEdit::singleline(&mut self.chat.session_filter)
                .hint_text("Filter sessions…")
                .desired_width(ui.available_width()),
        );

        if let Some(s) = self.selected_session_item() {
            if s.asd_session_id.is_none() && ui.small_button("Promote to ASD").clicked() {
                self.promote_selected();
            }
        }

        let needle = self.chat.session_filter.trim().to_lowercase();
        let mut pick = None;
        ScrollArea::vertical().id_salt("sessions").show(ui, |ui| {
            if self.sessions.is_empty() {
                ui.label(RichText::new("No sessions in this workspace").small().color(p.muted));
            }
            for (i, s) in self.sessions.iter().enumerate() {
                if !needle.is_empty() {
                    let matches_summary = s.summary.as_deref().map(|t| t.to_lowercase().contains(&needle)).unwrap_or(false);
                    let matches_harness = s.harness.to_lowercase().contains(&needle);
                    let matches_model = s.model.as_deref().map(|m| m.to_lowercase().contains(&needle)).unwrap_or(false);
                    if !matches_summary && !matches_harness && !matches_model {
                        continue;
                    }
                }
                let sel = self.selected_session.as_ref() == Some(&(s.harness.clone(), s.harness_session_id.clone()));
                let title = s
                    .summary
                    .as_deref()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.chars().take(46).collect::<String>())
                    .unwrap_or_else(|| short_id(&s.harness_session_id));

                let card_fill = if sel { p.raised } else { p.surface };
                let card_stroke = if sel {
                    Stroke::new(1.5_f32, p.accent)
                } else {
                    Stroke::new(1.0_f32, p.border)
                };

                Frame::none()
                    .fill(card_fill)
                    .stroke(card_stroke)
                    .inner_margin(Margin::symmetric(8.0, 6.0))
                    .rounding(Rounding::same(p.card_rounding))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let resp = ui.selectable_label(sel, RichText::new(title).strong().color(p.text_hi));
                        if resp.clicked() {
                            pick = Some(i);
                        }
                        ui.horizontal(|ui| {
                            if s.is_promoted_to_asd || s.asd_session_id.is_some() {
                                badge(ui, "ASD", theme::status_badge_for(tm, "done"));
                            }
                            ui.label(RichText::new(&s.harness).small().color(p.dim));
                            if let Some(m) = &s.model {
                                ui.label(RichText::new(format!("· {m}")).small().color(p.muted));
                            }
                        });
                    });
                ui.add_space(4.0);
            }
        });
        if let Some(i) = pick {
            self.select_session(i);
        }
    }

    // ---------------------------------------------------------------- Antigravity Chat view

    fn chat_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;

        // Trigger `@` file listing lazily if connected and typing `@`.
        if let Some((_, q)) = extract_at_query(&self.chat.composer) {
            let q_owned = q.to_string();
            if self.chat.fs_entries.is_empty() && self.chat.last_at_prefix.as_deref() != Some(q_owned.as_str()) {
                self.chat.last_at_prefix = Some(q_owned);
                if self.backend.is_connected() {
                    self.refresh_fs_list(None);
                }
            }
        } else {
            self.chat.last_at_prefix = None;
        }

        let slash_items = self.current_slash_items();
        let at_items = if slash_items.is_empty() { self.current_at_items() } else { Vec::new() };
        let popup_rows = slash_items.len().max(at_items.len()).min(8);
        let popup_h = if popup_rows > 0 { (popup_rows as f32 * 28.0) + 34.0 } else { 0.0 };
        let composer_card_h = 152.0;
        let timeline_h = (ui.available_height() - composer_card_h - popup_h - 12.0).max(80.0);

        let avail_w = ui.available_width();
        let canvas_w = avail_w.min(920.0);
        let side_pad = ((avail_w - canvas_w) * 0.5).max(0.0);

        // 1. Centered Antigravity Timeline Canvas
        ScrollArea::vertical()
            .id_salt("timeline")
            .max_height(timeline_h)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if side_pad > 0.0 {
                        ui.add_space(side_pad);
                    }
                    ui.vertical(|ui| {
                        ui.set_width(canvas_w);
                        if self.chat.timeline.entries.is_empty() {
                            ui.add_space(24.0);
                            Frame::none()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .inner_margin(Margin::same(16.0))
                                .rounding(Rounding::same(p.card_rounding))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(format!("Antigravity Chat · {}", self.chat.harness()))
                                            .strong()
                                            .size(16.0)
                                            .color(p.white),
                                    );
                                    ui.label(
                                        RichText::new("Type a message below, use / for Nexus & harness commands, @ to reference workspace files, or Ctrl+P for the Command Palette.")
                                            .small()
                                            .color(p.dim),
                                    );
                                });
                        }
                        let total_entries = self.chat.timeline.entries.len();
                        let turn_usage = self.chat.timeline.turn_usage;
                        let total_usage = self.chat.timeline.total_usage;
                        let turn_id = self.chat.timeline.turn_id.clone();
                        for (idx, e) in self.chat.timeline.entries.iter().enumerate() {
                            let is_last = idx + 1 == total_entries;
                            entry_ui(ui, e, tm, p, is_last, &turn_usage, &total_usage, turn_id.as_deref());
                        }
                        if self.chat.timeline.streaming {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Generating response…").small().color(p.dim));
                            });
                        }
                    });
                });
            });

        // 2. Centered Popup + Floating Rounded Composer Card
        let mut picked_slash: Option<SlashPopupItem> = None;
        let mut picked_at: Option<String> = None;
        let mut send = false;
        let mut manual_harness_change: Option<usize> = None;

        ui.horizontal(|ui| {
            if side_pad > 0.0 {
                ui.add_space(side_pad);
            }
            ui.vertical(|ui| {
                ui.set_width(canvas_w);

                // Inline `/` or `@` Autocomplete Popup above the composer
                if !slash_items.is_empty() {
                    let count = slash_items.len();
                    if self.chat.popup_selected >= count {
                        self.chat.popup_selected = 0;
                    }
                    Frame::none()
                        .fill(p.panel)
                        .stroke(Stroke::new(1.0_f32, p.border_hi))
                        .inner_margin(Margin::same(8.0))
                        .rounding(Rounding::same(p.card_rounding))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(format!("SLASH COMMANDS ({count}) · Tab/Click to select"))
                                    .small()
                                    .strong()
                                    .color(p.muted),
                            );
                            ScrollArea::vertical()
                                .id_salt("slash_popup")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for (i, item) in slash_items.iter().enumerate() {
                                        let sel = i == self.chat.popup_selected;
                                        ui.horizontal(|ui| {
                                            let b_style = if item.is_nexus {
                                                theme::status_badge_for(tm, "nexus")
                                            } else {
                                                theme::status_badge_for(tm, "done")
                                            };
                                            badge(ui, &item.badge, b_style);
                                            let cmd_lbl = RichText::new(format!("/{}", item.name))
                                                .monospace()
                                                .strong()
                                                .color(if sel { p.white } else { p.text_hi });
                                            if ui.selectable_label(sel, cmd_lbl).clicked() {
                                                picked_slash = Some(item.clone());
                                            }
                                            ui.label(RichText::new(&item.description).small().color(p.dim));
                                        });
                                    }
                                });
                        });
                    ui.add_space(4.0);
                } else if !at_items.is_empty() {
                    let count = at_items.len();
                    if self.chat.popup_selected >= count {
                        self.chat.popup_selected = 0;
                    }
                    Frame::none()
                        .fill(p.panel)
                        .stroke(Stroke::new(1.0_f32, p.border_hi))
                        .inner_margin(Margin::same(8.0))
                        .rounding(Rounding::same(p.card_rounding))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(format!("WORKSPACE FILES ({count}) · Tab/Click to insert @path"))
                                    .small()
                                    .strong()
                                    .color(p.muted),
                            );
                            ScrollArea::vertical()
                                .id_salt("at_popup")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for (i, entry) in at_items.iter().enumerate() {
                                        let sel = i == self.chat.popup_selected;
                                        ui.horizontal(|ui| {
                                            badge(ui, "file", theme::status_badge_for(tm, "pending"));
                                            let path_lbl = RichText::new(format!("@{}", entry.path))
                                                .monospace()
                                                .strong()
                                                .color(if sel { p.white } else { p.text_hi });
                                            if ui.selectable_label(sel, path_lbl).clicked() {
                                                picked_at = Some(entry.path.clone());
                                            }
                                        });
                                    }
                                });
                        });
                    ui.add_space(4.0);
                }

                // Floating Rounded Composer Card
                let focused = ui.ctx().memory(|m| m.has_focus(egui::Id::new("chat-composer")));
                let border_stroke = if focused {
                    Stroke::new(1.5_f32, p.accent)
                } else {
                    Stroke::new(1.0_f32, p.border_hi)
                };

                Frame::none()
                    .fill(p.surface)
                    .stroke(border_stroke)
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .rounding(Rounding::same(10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ScrollArea::vertical()
                            .id_salt("composer")
                            .max_height(88.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let resp = ui.add(
                                    TextEdit::multiline(&mut self.chat.composer)
                                        .id(egui::Id::new("chat-composer"))
                                        .frame(false)
                                        .hint_text("Ask anything… (/ for commands, @ for files, Enter to send, Shift+Enter for newline)")
                                        .desired_rows(3)
                                        .desired_width(ui.available_width())
                                        .return_key(Some(egui::KeyboardShortcut::new(
                                            egui::Modifiers::SHIFT,
                                            egui::Key::Enter,
                                        ))),
                                );
                                if resp.has_focus() {
                                    let popup_len = slash_items.len().max(at_items.len());
                                    ui.input_mut(|i| {
                                        let mut picked = false;
                                        if popup_len > 0 {
                                            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                                                self.chat.popup_selected = (self.chat.popup_selected + 1) % popup_len;
                                            }
                                            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                                                self.chat.popup_selected =
                                                    (self.chat.popup_selected + popup_len - 1) % popup_len;
                                            }
                                            if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                                                || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                            {
                                                if !slash_items.is_empty() {
                                                    picked_slash = slash_items.get(self.chat.popup_selected).cloned();
                                                    picked = picked_slash.is_some();
                                                } else if !at_items.is_empty() {
                                                    picked_at =
                                                        at_items.get(self.chat.popup_selected).map(|e| e.path.clone());
                                                    picked = picked_at.is_some();
                                                }
                                            }
                                        }
                                        if (!picked && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                                            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                                        {
                                            send = true;
                                        }
                                    });
                                }
                            });

                        ui.add_space(4.0);
                        ui.separator();

                        // Composer bottom internal toolbar
                        let streaming = self.chat.is_streaming();
                        let session_started = self.is_session_started();
                        ui.horizontal(|ui| {
                            // 1) [Harness ▾] dropdown pill (new draft) OR read-only badge (started session)
                            if !session_started {
                                let mut h_idx = self.chat.harness_idx;
                                egui::ComboBox::from_id_salt("harness")
                                    .selected_text(RichText::new(self.chat.harness()).small().strong().color(p.text_hi))
                                    .width(126.0)
                                    .show_ui(ui, |ui| {
                                        for (i, h) in HARNESSES.iter().enumerate() {
                                            ui.selectable_value(&mut h_idx, i, *h);
                                        }
                                    });
                                if h_idx != self.chat.harness_idx {
                                    manual_harness_change = Some(h_idx);
                                }
                            } else {
                                Frame::none()
                                    .fill(p.panel)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .inner_margin(Margin::symmetric(8.0, 4.0))
                                    .rounding(Rounding::same(p.pill_rounding))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(self.chat.harness())
                                                .small()
                                                .strong()
                                                .color(p.text_hi),
                                        );
                                    })
                                    .response
                                    .on_hover_text("Harness is locked for started session. Click + New Chat (/new) to pick another harness.");
                            }

                            // 2) [Model ▾ (searchable)] dropdown pill + ↻ refresh
                            let harness = self.chat.harness();
                            let options: Vec<String> =
                                self.chat.picker.filtered(harness).into_iter().map(String::from).collect();
                            let total = self.chat.picker.models.get(harness).map(Vec::len).unwrap_or(0);
                            let shown = self.chat.picker.selected.clone().unwrap_or_else(|| "(default model)".into());
                            let mut model_picked = false;
                            egui::ComboBox::from_id_salt("model")
                                .selected_text(RichText::new(shown).small().color(p.text))
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let search_w = (ui.available_width() - 56.0).max(80.0);
                                        ui.add(
                                            TextEdit::singleline(&mut self.chat.picker.filter)
                                                .hint_text("search models…")
                                                .desired_width(search_w),
                                        );
                                        if !self.chat.picker.filter.is_empty() && ui.small_button("Clear").clicked() {
                                            self.chat.picker.filter.clear();
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!("{} / {total}", options.len())).small().color(p.muted),
                                    );
                                    ScrollArea::vertical().id_salt("model-list").max_height(240.0).show(ui, |ui| {
                                        if ui
                                            .selectable_value(&mut self.chat.picker.selected, None, "(default model)")
                                            .clicked()
                                        {
                                            model_picked = true;
                                        }
                                        for m in options {
                                            if ui
                                                .selectable_value(&mut self.chat.picker.selected, Some(m.clone()), m)
                                                .clicked()
                                            {
                                                model_picked = true;
                                            }
                                        }
                                    });
                                });
                            if model_picked {
                                self.chat.picker.filter.clear();
                            }
                            if ui
                                .small_button("↻")
                                .on_hover_text("Refresh models list (/models)")
                                .clicked()
                            {
                                self.set_status("refreshing models…");
                                self.request(Request::Models { refresh: true });
                            }

                            // 3) Current directory pill (/cd)
                            let cur_dir = basename(&self.project_root);
                            if ui
                                .small_button(RichText::new(format!("📁 {cur_dir}")).color(p.dim))
                                .on_hover_text(format!("{}\nClick to switch folder (/cd)", self.project_root))
                                .clicked()
                            {
                                self.palette_open = true;
                                self.palette_query.clear();
                                self.palette_selected = 0;
                            }

                            // 4) Auto-Approve toggle pill (/auto-approve)
                            let auto_label = if self.chat.auto_approve { "Auto: ON" } else { "Auto: OFF" };
                            let auto_st = if self.chat.auto_approve {
                                theme::status_badge_for(tm, "done")
                            } else {
                                theme::status_badge_for(tm, "review")
                            };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(auto_label).small().strong().color(auto_st.fg))
                                        .fill(auto_st.bg)
                                        .stroke(auto_st.stroke)
                                        .rounding(Rounding::same(p.pill_rounding)),
                                )
                                .on_hover_text("Toggle auto-approve tool permissions (/auto-approve)")
                                .clicked()
                            {
                                self.set_auto_approve(!self.chat.auto_approve);
                            }

                            // 4) Right-aligned [Send] / [Stop] pill button
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if streaming {
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("  Stop  ").strong().color(p.white))
                                                .fill(p.raised)
                                                .rounding(Rounding::same(p.pill_rounding)),
                                        )
                                        .clicked()
                                    {
                                        self.cancel_turn();
                                    }
                                } else if ui
                                    .add(
                                        egui::Button::new(RichText::new("  Send  ").strong().color(p.white))
                                            .fill(p.accent_fill)
                                            .rounding(Rounding::same(p.pill_rounding)),
                                    )
                                    .clicked()
                                {
                                    send = true;
                                }
                            });
                        });
                    });
            });
        });

        if let Some(new_idx) = manual_harness_change {
            self.change_harness_manually(new_idx);
        }
        if let Some(item) = picked_slash {
            let ctx = ui.ctx().clone();
            self.apply_slash_popup_item(&item, Some(&ctx));
        }
        if let Some(path) = picked_at {
            self.chat.composer = replace_at_token(&self.chat.composer, &path);
            self.chat.popup_selected = 0;
        }
        if send {
            let ctx = ui.ctx().clone();
            if !self.try_handle_nexus_slash_command(&self.chat.composer.clone(), Some(&ctx)) {
                self.send_prompt();
            }
        }
    }

    // ---------------------------------------------------------------- Command Palette (Ctrl+P / Ctrl+K)

    pub fn build_palette_entries(&self) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();
        entries.push(PaletteEntry {
            category: "Mode",
            label: "Switch to Chat (Ctrl+1)".into(),
            detail: "Focus pure Antigravity Chat view".into(),
            action: PaletteAction::SwitchMode(PrimaryMode::Chat),
        });
        entries.push(PaletteEntry {
            category: "Mode",
            label: "Switch to Dashboard (Ctrl+2)".into(),
            detail: "Open PLAN, Diff, Subagents, Routing & Terminal".into(),
            action: PaletteAction::SwitchMode(PrimaryMode::Dashboard),
        });
        entries.push(PaletteEntry {
            category: "Theme",
            label: format!("Toggle Theme (current: {})", self.theme_mode.label()),
            detail: "Switch between Antigravity Dark and strict Monochrome".into(),
            action: PaletteAction::ToggleTheme,
        });
        for t in Tab::DASHBOARD {
            entries.push(PaletteEntry {
                category: "Dashboard",
                label: format!("Dashboard: {}", t.label()),
                detail: "Open orchestration tab".into(),
                action: PaletteAction::SwitchDashboardTab(t),
            });
        }
        if !self.is_session_started() {
            for (i, h) in HARNESSES.iter().enumerate() {
                entries.push(PaletteEntry {
                    category: "Harness",
                    label: format!("Harness: {h}"),
                    detail: "Switch active harness for new session".into(),
                    action: PaletteAction::SwitchHarness(i),
                });
            }
        }
        for p in &self.projects {
            entries.push(PaletteEntry {
                category: "Project",
                label: format!("Project: {}", basename(&p.project_root)),
                detail: p.project_root.clone(),
                action: PaletteAction::SwitchProject(p.project_root.clone()),
            });
        }
        for (i, s) in self.sessions.iter().enumerate() {
            let title = s
                .summary
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .map(|t| t.chars().take(48).collect::<String>())
                .unwrap_or_else(|| short_id(&s.harness_session_id));
            entries.push(PaletteEntry {
                category: "Session",
                label: format!("Session: {title}"),
                detail: format!("{} · {}", s.harness, s.model.as_deref().unwrap_or("-")),
                action: PaletteAction::SelectSession(i),
            });
        }
        for (h_idx, h) in HARNESSES.iter().enumerate() {
            if self.is_session_started() && h_idx != self.chat.harness_idx {
                continue;
            }
            if let Some(models) = self.chat.picker.models.get(*h) {
                for m in models {
                    entries.push(PaletteEntry {
                        category: "Model",
                        label: format!("Model: {m}"),
                        detail: (*h).to_string(),
                        action: PaletteAction::SelectModel(h_idx, Some(m.clone())),
                    });
                }
            }
        }
        for item in build_slash_items(
            "",
            self.chat.harness(),
            &self.chat.harness_commands,
            !self.is_session_started(),
        ) {
            entries.push(PaletteEntry {
                category: "Command",
                label: format!("/{}", item.name),
                detail: format!("[{}] {}", item.badge, item.description),
                action: PaletteAction::SlashCommand(item),
            });
        }
        let needle = self.palette_query.trim().to_lowercase();
        if needle.is_empty() {
            return entries;
        }
        entries
            .into_iter()
            .filter(|e| {
                e.label.to_lowercase().contains(&needle)
                    || e.detail.to_lowercase().contains(&needle)
                    || e.category.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn execute_palette_action(&mut self, action: PaletteAction, ctx: Option<&egui::Context>) {
        self.palette_open = false;
        match action {
            PaletteAction::SwitchMode(m) => self.set_mode(m),
            PaletteAction::SwitchDashboardTab(t) => self.select_dashboard_tab(t),
            PaletteAction::ToggleTheme => {
                if let Some(c) = ctx {
                    self.toggle_theme(c);
                } else {
                    self.theme_mode = self.theme_mode.toggle();
                    self.applied_theme = None;
                }
            }
            PaletteAction::SwitchHarness(idx) => self.change_harness_manually(idx),
            PaletteAction::SelectModel(h_idx, model) => {
                if h_idx != self.chat.harness_idx {
                    if self.is_session_started() {
                        self.set_status("harness is locked for started session; click + New Chat (/new) first");
                        return;
                    }
                    self.change_harness_manually(h_idx);
                }
                self.chat.picker.selected = model;
            }
            PaletteAction::SwitchProject(root) => self.switch_project(root),
            PaletteAction::SelectSession(idx) => self.select_session(idx),
            PaletteAction::SlashCommand(item) => self.apply_slash_popup_item(&item, ctx),
        }
    }

    fn command_palette_modal(&mut self, ctx: &egui::Context) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        let entries = self.build_palette_entries();
        if self.palette_selected >= entries.len() && !entries.is_empty() {
            self.palette_selected = 0;
        }
        let mut chosen: Option<PaletteAction> = None;
        let mut close = false;

        egui::Window::new("Command Palette")
            .id(egui::Id::new("command-palette-window"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0))
            .fixed_size(egui::vec2(600.0, 380.0))
            .frame(
                Frame::none()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.5_f32, p.border_hi))
                    .inner_margin(Margin::same(12.0))
                    .rounding(Rounding::same(p.card_rounding)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.palette_query)
                            .id(egui::Id::new("command-palette-input"))
                            .hint_text("Type a command, project, session, harness, or model…")
                            .desired_width(ui.available_width() - 60.0),
                    );
                    resp.request_focus();
                    if ui.small_button("Esc").clicked() {
                        close = true;
                    }
                });
                ui.separator();
                ScrollArea::vertical()
                    .id_salt("palette-scroll")
                    .max_height(310.0)
                    .show(ui, |ui| {
                        for (i, entry) in entries.iter().enumerate() {
                            let sel = i == self.palette_selected;
                            ui.horizontal(|ui| {
                                badge(ui, entry.category, theme::status_badge_for(tm, "pending"));
                                if ui
                                    .selectable_label(
                                        sel,
                                        RichText::new(&entry.label)
                                            .strong()
                                            .color(if sel { p.white } else { p.text_hi }),
                                    )
                                    .clicked()
                                {
                                    chosen = Some(entry.action.clone());
                                }
                                ui.label(RichText::new(&entry.detail).small().color(p.muted));
                            });
                        }
                    });
            });

        if close {
            self.palette_open = false;
        }
        if let Some(act) = chosen {
            self.execute_palette_action(act, Some(ctx));
        }
    }

    fn permission_request_modal(&mut self, ctx: &egui::Context) {
        let Some((turn_id, perm)) = self.chat.pending_permissions.front().cloned() else {
            return;
        };
        let queue_len = self.chat.pending_permissions.len();
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        let mut decision: Option<(&'static str, Option<String>)> = None;

        egui::Window::new("Permission Request")
            .id(egui::Id::new("permission-request-modal"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(520.0)
            .max_width(560.0)
            .frame(
                Frame::none()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.5_f32, p.border_hi))
                    .inner_margin(Margin::same(14.0))
                    .rounding(Rounding::same(p.card_rounding)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    badge(ui, "PERMISSION", theme::status_badge_for(tm, "review"));
                    if queue_len > 1 {
                        ui.label(
                            RichText::new(format!("1 of {queue_len} pending"))
                                .small()
                                .color(p.dim),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !turn_id.is_empty() {
                            ui.label(
                                RichText::new(short_id(&turn_id))
                                    .small()
                                    .monospace()
                                    .color(p.muted),
                            );
                        }
                    });
                });
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Tool:").small().strong().color(p.dim));
                    ui.label(RichText::new(&perm.tool_name).monospace().strong().color(p.white));
                    if let Some(kind) = &perm.tool_kind {
                        ui.label(RichText::new(format!("({kind})")).small().color(p.muted));
                    }
                });

                if let Some(locs) = perm.locations.as_ref().filter(|v| !v.is_empty()) {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Paths:").small().strong().color(p.dim));
                        ui.label(RichText::new(locs.join(", ")).monospace().small().color(p.text_hi));
                    });
                }

                if let Some(input) = perm.tool_input.as_ref().filter(|s| !s.trim().is_empty()) {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Input:").small().strong().color(p.dim));
                    Frame::none()
                        .fill(p.code_bg)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .inner_margin(Margin::same(8.0))
                        .rounding(Rounding::same(6.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ScrollArea::vertical()
                                .id_salt("perm-input-scroll")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.label(RichText::new(input).monospace().small().color(p.text));
                                });
                        });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    if perm.options.is_empty() {
                        let allow_st = theme::status_badge_for(tm, "done");
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Allow Once").strong().color(allow_st.fg))
                                    .fill(allow_st.bg)
                                    .stroke(allow_st.stroke)
                                    .rounding(Rounding::same(p.pill_rounding)),
                            )
                            .clicked()
                        {
                            decision = Some(("selected", Some("allow_once".into())));
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Allow Always").strong().color(p.white))
                                    .fill(p.accent_fill)
                                    .rounding(Rounding::same(p.pill_rounding)),
                            )
                            .clicked()
                        {
                            decision = Some(("selected", Some("allow_always".into())));
                        }
                        let rej_st = theme::status_badge_for(tm, "failed");
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Reject").strong().color(rej_st.fg))
                                    .fill(rej_st.bg)
                                    .stroke(rej_st.stroke)
                                    .rounding(Rounding::same(p.pill_rounding)),
                            )
                            .clicked()
                        {
                            decision = Some(("cancelled", None));
                        }
                    } else {
                        let has_reject = perm.options.iter().any(|o| o.kind.starts_with("reject"));
                        for opt in &perm.options {
                            let btn = match opt.kind.as_str() {
                                "allow_always" => egui::Button::new(
                                    RichText::new(&opt.name).strong().color(p.white),
                                )
                                .fill(p.accent_fill)
                                .rounding(Rounding::same(p.pill_rounding)),
                                k if k.starts_with("reject") => {
                                    let st = theme::status_badge_for(tm, "failed");
                                    egui::Button::new(RichText::new(&opt.name).strong().color(st.fg))
                                        .fill(st.bg)
                                        .stroke(st.stroke)
                                        .rounding(Rounding::same(p.pill_rounding))
                                }
                                _ => {
                                    let st = theme::status_badge_for(tm, "done");
                                    egui::Button::new(RichText::new(&opt.name).strong().color(st.fg))
                                        .fill(st.bg)
                                        .stroke(st.stroke)
                                        .rounding(Rounding::same(p.pill_rounding))
                                }
                            };
                            if ui.add(btn).clicked() {
                                decision = Some(("selected", Some(opt.option_id.clone())));
                            }
                        }
                        if !has_reject {
                            let rej_st = theme::status_badge_for(tm, "failed");
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Reject").strong().color(rej_st.fg))
                                        .fill(rej_st.bg)
                                        .stroke(rej_st.stroke)
                                        .rounding(Rounding::same(p.pill_rounding)),
                                )
                                .clicked()
                            {
                                decision = Some(("cancelled", None));
                            }
                        }
                    }
                });
            });

        if let Some((outcome, option_id)) = decision {
            self.respond_to_permission(outcome, option_id);
        }
    }

    // ---------------------------------------------------------------- board

    fn board_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_board();
            }
            if ui.button("Sync PLAN → DB").clicked() {
                self.request(Request::SyncPlan(PlanSyncRequest {
                    project_root: self.root_opt(),
                    session_id: self.session_for_actions(),
                    plan_md_path: (!self.plan.path.is_empty()).then(|| self.plan.path.clone()),
                }));
            }
            ui.separator();
            ui.checkbox(&mut self.board.dispatch_claim_only, "claim only");
            if ui.button("Dispatch").clicked() {
                match self.session_for_actions() {
                    Some(sid) => {
                        let turn = self.board.board.as_ref().and_then(|b| b.tasks.iter().filter_map(|t| t.turn).max());
                        self.request(Request::Dispatch(DispatchRequest {
                            project_root: self.root_opt(),
                            session_id: sid,
                            turn,
                            mode: Some(if self.board.dispatch_claim_only { "claim_only" } else { "parallel_spawn" }.into()),
                            task_ids: self.board.selected_task.clone().map(|t| vec![t]),
                        }));
                    }
                    None => self.set_status("dispatch: no session"),
                }
            }
            if let Some(b) = &self.board.board {
                ui.separator();
                ui.label(RichText::new(format!(
                    "{} · phase {} · {}",
                    b.session_id.as_deref().map(short_id).unwrap_or_else(|| "-".into()),
                    b.phase.as_deref().unwrap_or("-"),
                    b.progress.as_deref().unwrap_or("")
                ))
                .small()
                .color(p.dim));
            }
        });
        egui::CollapsingHeader::new("Debate").id_salt("debate").show(ui, |ui| {
            ui.add(TextEdit::singleline(&mut self.board.debate_topic).hint_text("topic").desired_width(f32::INFINITY));
            ui.add(TextEdit::multiline(&mut self.board.debate_proposal).hint_text("proposal").desired_rows(3).desired_width(f32::INFINITY));
            if ui.button("Run Debate").clicked() {
                match self.session_for_actions() {
                    Some(sid) => self.request(Request::Debate(DebateRequest {
                        project_root: self.root_opt(),
                        session_id: sid,
                        topic: self.board.debate_topic.clone(),
                        proposal: self.board.debate_proposal.clone(),
                    })),
                    None => self.set_status("debate: no session"),
                }
            }
        });
        if let Some(r) = &self.board.last_result {
            ui.label(RichText::new(r).small().color(p.text_hi));
        }

        // Kanban
        let mut open_diff = None;
        let mut select = None;
        let tasks = self.board.board.as_ref().map(|b| b.tasks.clone()).unwrap_or_default();
        let cols = group_kanban(&tasks);
        let selected = self.board.selected_task.clone();
        ui.columns(KANBAN_COLUMNS.len(), |c| {
            for (i, col) in cols.iter().enumerate() {
                let ui = &mut c[i];
                ui.label(RichText::new(format!("{} · {}", KANBAN_COLUMNS[i], col.len())).strong().color(p.text_hi));
                ScrollArea::vertical().id_salt(("kanban", i)).max_height(220.0).show(ui, |ui| {
                    for t in col {
                        let sel = selected.as_deref() == Some(t.id.as_str());
                        Frame::none()
                            .fill(if sel { p.raised } else { p.surface })
                            .stroke(Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { p.accent } else { p.border }))
                            .inner_margin(Margin::same(6.0))
                            .rounding(Rounding::same(p.card_rounding))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(sel, RichText::new(&t.id).monospace().strong()).clicked() {
                                        select = Some(t.id.clone());
                                    }
                                    badge(ui, &t.status, theme::status_badge_for(tm, &t.status));
                                });
                                ui.label(RichText::new(&t.subject).small());
                                let meta = [t.assignee_role.clone(), t.difficulty.clone(), t.turn.map(|n| format!("turn {n}"))]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                if !meta.is_empty() {
                                    ui.label(RichText::new(meta).small().color(p.muted));
                                }
                                if ui.small_button("Diff").clicked() {
                                    open_diff = Some((t.id.clone(), t.turn));
                                }
                            });
                        ui.add_space(4.0);
                    }
                });
            }
        });
        if let Some(id) = select {
            self.board.selected_task = if self.board.selected_task.as_deref() == Some(&id) { None } else { Some(id) };
        }
        if let Some((id, turn)) = open_diff {
            self.diff.task_id = id;
            self.diff.turn = turn.map(|t| t.to_string()).unwrap_or_default();
            self.select_dashboard_tab(Tab::Diff);
            self.load_diff();
        }

        ui.separator();
        self.plan_editor(ui);
    }

    fn plan_editor(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            ui.label(RichText::new("PLAN.md").strong().color(p.white));
            ui.label(RichText::new(if self.plan.path.is_empty() { "-" } else { &self.plan.path }).small().monospace().color(p.dim));
            if self.plan.is_dirty() {
                badge(ui, "UNSAVED", theme::status_badge_for(tm, "unsaved"));
            }
            if !self.plan.exists && self.plan.loaded {
                badge(ui, "NEW FILE", theme::status_badge_for(tm, "pending"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dirty = self.plan.is_dirty();
                if ui.add_enabled(dirty && self.plan.conflict.is_none(), egui::Button::new("Save")).clicked() {
                    self.save_plan();
                }
                if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                    self.plan.buffer = self.plan.base.clone();
                }
                if ui.button("Reload").clicked() {
                    self.load_plan(false);
                }
                ui.label(
                    RichText::new(format!(
                        "sha {} · mtime {}",
                        self.plan.sha256.as_deref().map(|s| &s[..s.len().min(10)]).unwrap_or("-"),
                        self.plan.mtime_ms.map(|m| format!("{m:.0}")).unwrap_or_else(|| "-".into())
                    ))
                    .small()
                    .monospace()
                    .color(p.muted),
                );
            });
        });
        if let Some(c) = self.plan.conflict.clone() {
            let mut take = false;
            let mut keep = false;
            Frame::none()
                .fill(p.bg)
                .stroke(Stroke::new(2.0_f32, p.white))
                .inner_margin(Margin::same(8.0))
                .rounding(Rounding::same(p.card_rounding))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        badge(ui, "CONFLICT", theme::status_badge_for(tm, "failed"));
                        ui.label(RichText::new(&c.reason).color(p.white));
                    });
                    ui.horizontal(|ui| {
                        if ui.button(if c.remote_content.is_some() { "Take disk version" } else { "Reload from disk" }).clicked() {
                            take = true;
                        }
                        if ui.button("Keep mine (overwrite on save)").clicked() {
                            keep = true;
                        }
                    });
                });
            if take && !self.plan.resolve_take_theirs() {
                self.plan.conflict = None;
                self.load_plan(true);
            }
            if keep {
                self.plan.resolve_keep_mine();
                self.set_status("conflict resolved: keeping local edits");
            }
        }
        ScrollArea::vertical().id_salt("plan_editor").auto_shrink([false, false]).show(ui, |ui| {
            ui.add(
                TextEdit::multiline(&mut self.plan.buffer)
                    .code_editor()
                    .desired_rows(16)
                    .desired_width(f32::INFINITY),
            );
        });
    }

    // ---------------------------------------------------------------- diff

    fn diff_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Task").color(p.dim));
            ui.add(TextEdit::singleline(&mut self.diff.task_id).desired_width(120.0).hint_text("T1.2"));
            ui.label(RichText::new("Turn").color(p.dim));
            ui.add(TextEdit::singleline(&mut self.diff.turn).desired_width(40.0));
            ui.checkbox(&mut self.diff.merge_base, "merge-base");
            if ui.button("Load Diff").clicked() {
                self.load_diff();
            }
        });
        if let Some(d) = &self.diff.resp {
            ui.label(
                RichText::new(format!(
                    "{} · mode {} · base {}",
                    d.worktree_path.as_deref().unwrap_or("-"),
                    d.mode.as_deref().unwrap_or("-"),
                    d.base_ref.as_deref().unwrap_or("-")
                ))
                .small()
                .monospace()
                .color(p.dim),
            );
            if let Some(e) = &d.error {
                badge(ui, &format!("ERROR {e}"), theme::status_badge_for(tm, "failed"));
            }
        }
        ui.separator();
        let task = self.diff.task_id.trim().to_string();
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.diff.approve_summary).hint_text("approve summary").desired_width(220.0));
            if ui.add_enabled(!task.is_empty(), egui::Button::new("Approve & Merge")).clicked() {
                let s = self.diff.approve_summary.trim().to_string();
                self.request(Request::Approve(ApproveTaskRequest {
                    project_root: self.root_opt(),
                    task_id: task.clone(),
                    summary: (!s.is_empty()).then_some(s),
                }));
            }
            ui.separator();
            ui.add(TextEdit::singleline(&mut self.diff.reject_reason).hint_text("reject reason").desired_width(220.0));
            if ui.add_enabled(!task.is_empty(), egui::Button::new("Reject")).clicked() {
                let r = self.diff.reject_reason.trim().to_string();
                self.request(Request::Reject(RejectTaskRequest {
                    project_root: self.root_opt(),
                    task_id: task.clone(),
                    reason: (!r.is_empty()).then_some(r),
                }));
            }
        });
        if let Some(r) = &self.diff.last_result {
            ui.label(RichText::new(r).small().color(p.text_hi));
        }
        ui.separator();
        let Some(d) = &self.diff.resp else {
            ui.label(RichText::new("No diff loaded.").color(p.muted));
            return;
        };
        let add_st = theme::diff_style_for(tm, theme::DiffLineKind::Add);
        let del_st = theme::diff_style_for(tm, theme::DiffLineKind::Del);
        egui::Grid::new("diff_files").striped(true).num_columns(4).show(ui, |ui| {
            for f in &d.files {
                ui.label(RichText::new(f.status.as_deref().unwrap_or("M")).monospace().strong());
                ui.label(RichText::new(&f.path).monospace());
                ui.label(RichText::new(format!("+{}", f.additions.unwrap_or(0))).monospace().color(add_st.fg));
                let mut del_rt = RichText::new(format!("-{}", f.deletions.unwrap_or(0))).monospace().color(del_st.fg);
                if del_st.strike {
                    del_rt = del_rt.strikethrough();
                }
                ui.label(del_rt);
                ui.end_row();
            }
        });
        ui.separator();
        let text = d.diff_text.clone().unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        ScrollArea::both().id_salt("diff_text").auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
            for line in &lines[range] {
                let st = theme::diff_style_for(tm, theme::classify_diff_line(line));
                let mut rt = RichText::new(*line).monospace().color(st.fg).background_color(st.bg);
                if st.strike {
                    rt = rt.strikethrough();
                }
                if st.strong {
                    rt = rt.strong();
                }
                ui.label(rt);
            }
        });
    }

    // ---------------------------------------------------------------- subagents

    fn subagents_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_subagents();
            }
            ui.checkbox(&mut self.subagents.auto_refresh, "live (1.5s)");
            ui.label(RichText::new(format!("{} subagents", self.subagents.list.len())).small().color(p.dim));
        });
        ui.separator();
        let mut pick = None;
        ScrollArea::vertical().id_salt("subagents").max_height(220.0).show(ui, |ui| {
            egui::Grid::new("subagent_grid").striped(true).num_columns(7).show(ui, |ui| {
                for h in ["sub-session", "status", "role", "harness", "model", "task", "exit"] {
                    ui.label(RichText::new(h).small().strong().color(p.dim));
                }
                ui.end_row();
                for s in &self.subagents.list {
                    let sel = self.subagents.selected.as_deref() == Some(s.sub_session_id.as_str());
                    if ui
                        .selectable_label(sel, RichText::new(short_id(&s.sub_session_id)).monospace())
                        .on_hover_text(&s.sub_session_id)
                        .clicked()
                    {
                        pick = Some(s.sub_session_id.clone());
                    }
                    badge(ui, &s.status, theme::status_badge_for(tm, &s.status));
                    ui.label(s.role.as_deref().unwrap_or("-"));
                    ui.label(s.harness.as_deref().unwrap_or("-"));
                    ui.label(RichText::new(s.model.as_deref().unwrap_or("-")).small());
                    ui.label(s.task_id.as_deref().unwrap_or("-"));
                    ui.label(s.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()));
                    ui.end_row();
                }
            });
        });
        if let Some(id) = pick {
            self.subagents.selected = Some(id.clone());
            self.subagents.logs.clear();
            self.request(Request::SubagentLogs(SubagentLogTailRequest {
                project_root: self.root_opt(),
                sub_session_id: id,
                tail_lines: Some(self.subagents.tail_lines),
            }));
        }
        ui.separator();
        let has_sel = self.subagents.selected.is_some();
        ui.add_enabled_ui(has_sel, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Kill").clicked() {
                    self.subagent_action(SubagentControlActionKind::Kill);
                }
                if ui.button("Retry").clicked() {
                    self.subagent_action(SubagentControlActionKind::Retry);
                }
                if ui.button("Resume").clicked() {
                    self.subagent_action(SubagentControlActionKind::Resume);
                }
                ui.separator();
                egui::ComboBox::from_id_salt("fallback_harness")
                    .selected_text(HARNESSES[self.subagents.fallback_harness])
                    .show_ui(ui, |ui| {
                        for (i, h) in HARNESSES.iter().enumerate() {
                            ui.selectable_value(&mut self.subagents.fallback_harness, i, *h);
                        }
                    });
                ui.add(TextEdit::singleline(&mut self.subagents.fallback_model).hint_text("fallback model").desired_width(160.0));
                if ui.button("Fallback").clicked() {
                    self.subagent_action(SubagentControlActionKind::Fallback);
                }
            });
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.subagents.custom_reason).hint_text("custom instruction / reason").desired_width(360.0));
                if ui.button("Custom").clicked() {
                    self.subagent_action(SubagentControlActionKind::Custom);
                }
            });
        });
        if let Some(r) = &self.subagents.last_result {
            ui.label(RichText::new(r).small().color(p.text_hi));
        }
        ui.separator();
        ui.label(RichText::new("LOG TAIL").small().strong().color(p.dim));
        Frame::none().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).inner_margin(Margin::same(6.0)).show(ui, |ui| {
            ScrollArea::both().id_salt("logtail").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                if self.subagents.logs.is_empty() {
                    ui.label(RichText::new("select a subagent to tail its output.log").small().color(p.muted));
                }
                for l in &self.subagents.logs {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        });
    }

    // ---------------------------------------------------------------- routing

    fn routing_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let tm = self.theme_mode;
        ui.horizontal(|ui| {
            if ui.button("Load").clicked() {
                self.request(Request::Routing { project_root: self.project_root.clone() });
            }
            let dirty = self.routing.loaded && self.routing.is_dirty();
            if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                match self.routing.build() {
                    Ok(v) => {
                        self.routing.error = None;
                        self.request(Request::SaveRouting(SaveRoutingRequest {
                            project_root: self.project_root.clone(),
                            action: "save".into(),
                            routing_config: Some(v),
                        }));
                    }
                    Err(e) => self.routing.error = Some(e),
                }
            }
            if ui.button("Reset to defaults").clicked() {
                self.request(Request::SaveRouting(SaveRoutingRequest {
                    project_root: self.project_root.clone(),
                    action: "reset".into(),
                    routing_config: None,
                }));
            }
            if dirty {
                badge(ui, "UNSAVED", theme::status_badge_for(tm, "unsaved"));
            }
        });
        if let Some(e) = &self.routing.error {
            badge(ui, e, theme::status_badge_for(tm, "failed"));
        }
        ui.separator();
        ScrollArea::vertical().id_salt("routing").auto_shrink([false, false]).show(ui, |ui| {
            for (i, key) in ROUTING_SECTIONS.iter().enumerate() {
                egui::CollapsingHeader::new(RichText::new(*key).strong().color(p.white))
                    .id_salt(("routing", i))
                    .default_open(true)
                    .show(ui, |ui| {
                        let rows = self.routing.summary(key);
                        if !rows.is_empty() {
                            egui::Grid::new(("routing_grid", i)).striped(true).num_columns(2).show(ui, |ui| {
                                for (k, v) in rows {
                                    ui.label(RichText::new(k).monospace().strong());
                                    ui.label(RichText::new(v).small());
                                    ui.end_row();
                                }
                            });
                        }
                        ui.add(
                            TextEdit::multiline(&mut self.routing.sections[i])
                                .code_editor()
                                .desired_rows(6)
                                .desired_width(f32::INFINITY)
                                .hint_text("JSON (empty = remove)"),
                        );
                    });
            }
        });
    }

    // ---------------------------------------------------------------- terminal

    fn terminal_tab(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(self.theme_mode);
        let session = self.selected_session_item().cloned();
        let sid = self.session_for_actions();
        egui::Grid::new("term_info").num_columns(2).show(ui, |ui| {
            ui.label(RichText::new("ASD session").color(p.dim));
            ui.label(RichText::new(sid.clone().unwrap_or_else(|| "-".into())).monospace());
            ui.end_row();
            ui.label(RichText::new("harness").color(p.dim));
            ui.label(session.as_ref().map(|s| s.harness.clone()).unwrap_or_else(|| self.chat.harness().into()));
            ui.end_row();
            ui.label(RichText::new("harness session").color(p.dim));
            ui.label(RichText::new(session.as_ref().map(|s| s.harness_session_id.clone()).unwrap_or_else(|| "-".into())).monospace());
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(sid.is_some(), egui::Button::new("Get resume command")).clicked() {
                self.fetch_resume();
            }
            egui::ComboBox::from_id_salt("terminal_kind")
                .selected_text(terminal_label(self.terminal.kind))
                .show_ui(ui, |ui| {
                    for k in [TerminalKind::WindowsTerminal, TerminalKind::Powershell, TerminalKind::Cmd] {
                        ui.selectable_value(&mut self.terminal.kind, k, terminal_label(k));
                    }
                });
        });
        ui.separator();
        if let Some(r) = self.terminal.resume.clone() {
            ui.label(RichText::new("INTERACTIVE RESUME COMMAND").small().strong().color(p.dim));
            let mut cli = r.cli_string.as_str();
            ui.add(TextEdit::multiline(&mut cli).code_editor().desired_rows(2).desired_width(f32::INFINITY));
            ui.label(RichText::new(format!("cwd: {}", r.cwd)).small().monospace().color(p.muted));
            ui.horizontal(|ui| {
                if ui.button("Copy").clicked() {
                    self.copy_resume(ui.ctx());
                }
                if ui.button("Launch OS terminal").clicked() {
                    self.request(Request::LaunchLocal { spec: resume_to_spec(&r), kind: Some(self.terminal.kind) });
                }
                if let Some(s) = sid.clone() {
                    if ui.button("Open via sidecar").clicked() {
                        self.request(Request::TerminalOpen {
                            req: TerminalOpenRequest {
                                project_root: self.project_root.clone(),
                                session_id: s,
                                harness: r.harness.clone(),
                                terminal: Some(self.terminal.kind),
                            },
                            kind: Some(self.terminal.kind),
                        });
                    }
                }
            });
        } else {
            ui.label(RichText::new("Select an ASD session and fetch its resume command.").color(p.muted));
        }
        if let Some(r) = &self.terminal.last_result {
            ui.label(RichText::new(r).small().color(p.text_hi));
        }
        ui.separator();
        ui.label(RichText::new("ACTIVITY").small().strong().color(p.dim));
        ScrollArea::vertical().id_salt("activity").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for l in &self.log {
                ui.label(RichText::new(l).small().monospace().color(p.text));
            }
        });
    }

    // ---------------------------------------------------------------- window helpers

    fn window_hooks(&mut self, ctx: &egui::Context) {
        let elapsed = self.started.elapsed();
        if let Some(limit) = self.run.exit_after {
            if elapsed >= limit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if let Some(path) = self.run.screenshot.clone() {
            let settled = matches!(self.conn, ConnState::Connected(_) | ConnState::Failed(_)) && self.backend.in_flight == 0;
            let ready = self.frames > 5 && elapsed > Duration::from_millis(1500) && (settled || elapsed > Duration::from_secs(20));
            if ready && !self.screenshot_requested {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let mut shot = None;
            ctx.input(|i| {
                for ev in &i.raw.events {
                    if let egui::Event::Screenshot { image, .. } = ev {
                        shot = Some(image.clone());
                    }
                }
            });
            if let Some(img) = shot {
                match crate::screenshot::save_color_image(&path, &img) {
                    Ok(()) => {
                        println!("screenshot saved: {}", path.display());
                        self.screenshot_saved = Some(path);
                    }
                    Err(e) => eprintln!("screenshot failed: {e}"),
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl eframe::App for NexusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
        self.window_hooks(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::palette(self.theme_mode).bg).to_array()
    }
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

fn terminal_label(k: TerminalKind) -> &'static str {
    match k {
        TerminalKind::WindowsTerminal => "Windows Terminal",
        TerminalKind::Powershell => "PowerShell",
        TerminalKind::Cmd => "CMD",
    }
}

fn panel_frame(fill: Color32, border: Color32) -> Frame {
    Frame::none().fill(fill).inner_margin(Margin::same(8.0)).stroke(Stroke::new(1.0_f32, border))
}

fn section_title(ui: &mut egui::Ui, t: &str, p: ThemePalette) {
    ui.label(RichText::new(t).small().strong().color(p.dim));
}

/// Pill badge.
pub fn badge(ui: &mut egui::Ui, text: &str, st: BadgeStyle) -> egui::Response {
    Frame::none()
        .fill(st.bg)
        .stroke(st.stroke)
        .inner_margin(Margin::symmetric(5.0, 1.0))
        .rounding(Rounding::same(4.0))
        .show(ui, |ui| ui.label(RichText::new(text.to_uppercase()).small().strong().color(st.fg)))
        .response
}

/// Render structured markdown-like text (fenced code blocks, headers, bullet lists).
fn render_assistant_markdown(ui: &mut egui::Ui, body: &str, p: ThemePalette) {
    let mut in_code = false;
    let mut code_buf = String::new();
    let mut code_lang = String::new();

    for line in body.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            if in_code {
                let code_text = code_buf.trim_end_matches('\n').to_string();
                Frame::none()
                    .fill(p.code_bg)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(Margin::same(8.0))
                    .rounding(Rounding::same(6.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if !code_lang.is_empty() {
                            ui.label(RichText::new(&code_lang).small().monospace().color(p.muted));
                        }
                        ui.label(RichText::new(code_text).monospace().color(p.text_hi));
                    });
                code_buf.clear();
                code_lang.clear();
                in_code = false;
            } else {
                in_code = true;
                code_lang = rest.trim().to_string();
            }
            continue;
        }
        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
            continue;
        }
        if let Some(h) = line.strip_prefix("### ") {
            ui.label(RichText::new(h).strong().size(16.0).color(p.white));
        } else if let Some(h) = line.strip_prefix("## ") {
            ui.label(RichText::new(h).strong().size(17.5).color(p.white));
        } else if let Some(h) = line.strip_prefix("# ") {
            ui.label(RichText::new(h).strong().size(19.0).color(p.white));
        } else if let Some(b) = line.trim_start().strip_prefix("- ").or_else(|| line.trim_start().strip_prefix("* ")) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").strong().color(p.accent));
                ui.label(RichText::new(b).color(p.text_hi));
            });
        } else {
            ui.label(RichText::new(line).color(p.text_hi));
        }
    }
    if in_code && !code_buf.is_empty() {
        Frame::none()
            .fill(p.code_bg)
            .stroke(Stroke::new(1.0_f32, p.border))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(code_buf.trim_end()).monospace().color(p.text_hi));
            });
    }
}

#[allow(clippy::too_many_arguments)]
fn entry_ui(
    ui: &mut egui::Ui,
    e: &Entry,
    tm: ThemeMode,
    p: ThemePalette,
    is_last: bool,
    turn_usage: &Usage,
    total_usage: &Usage,
    turn_id: Option<&str>,
) {
    match e.kind {
        EntryKind::User => {
            ui.horizontal(|ui| {
                let indent = (ui.available_width() * 0.14).max(0.0);
                ui.add_space(indent);
                Frame::none()
                    .fill(p.user_bubble)
                    .stroke(Stroke::new(1.0_f32, p.user_border))
                    .inner_margin(Margin::symmetric(12.0, 9.0))
                    .rounding(Rounding::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("YOU").small().strong().color(p.accent));
                        ui.label(RichText::new(&e.body).color(p.white));
                    });
            });
        }
        EntryKind::Error => {
            Frame::none()
                .fill(p.bg)
                .stroke(Stroke::new(1.5_f32, p.white))
                .inner_margin(Margin::same(10.0))
                .rounding(Rounding::same(p.card_rounding))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        badge(ui, "ERROR", theme::status_badge_for(tm, "failed"));
                        if !e.title.is_empty() {
                            ui.label(RichText::new(&e.title).small().color(p.dim));
                        }
                    });
                    ui.label(RichText::new(&e.body).color(p.white));
                });
        }
        EntryKind::System => {
            ui.label(RichText::new(format!("— {} —", e.body)).small().italics().color(p.muted));
        }
        EntryKind::Thinking => {
            let id = ui.make_persistent_id(("entry", e.id));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
            Frame::none()
                .fill(p.panel)
                .stroke(Stroke::new(1.0_f32, p.border))
                .inner_margin(Margin::symmetric(8.0, 4.0))
                .rounding(Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    state
                        .show_header(ui, |ui| {
                            ui.label(RichText::new(e.thought_header()).small().italics().color(p.dim));
                        })
                        .body(|ui| {
                            ui.label(RichText::new(&e.body).italics().color(p.dim));
                        });
                });
        }
        EntryKind::ToolUse | EntryKind::ToolResult => {
            let id = ui.make_persistent_id(("entry", e.id));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
            let stroke = if e.is_error {
                Stroke::new(1.5_f32, p.white)
            } else {
                Stroke::new(1.0_f32, p.border)
            };
            Frame::none()
                .fill(p.surface)
                .stroke(stroke)
                .inner_margin(Margin::symmetric(8.0, 5.0))
                .rounding(Rounding::same(p.card_rounding))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    state
                        .show_header(ui, |ui| {
                            let icon = if e.kind == EntryKind::ToolUse {
                                "⚙"
                            } else if e.is_error {
                                "✗"
                            } else {
                                "✓"
                            };
                            ui.label(RichText::new(icon).small().strong().color(p.accent));
                            ui.label(RichText::new(e.kind.label()).small().strong().color(p.dim));
                            if !e.title.is_empty() {
                                ui.label(RichText::new(&e.title).monospace().strong().color(p.text_hi));
                            }
                            if e.pending {
                                ui.spinner();
                            }
                            if e.is_error {
                                badge(ui, "ERROR", theme::status_badge_for(tm, "failed"));
                            }
                            let summary = e.compact_summary(52);
                            if !summary.is_empty() {
                                ui.label(RichText::new(summary).small().monospace().color(p.muted));
                            }
                        })
                        .body(|ui| {
                            let body = if e.body.chars().count() > 8000 {
                                e.body.chars().take(8000).collect::<String>() + "\n… (truncated)"
                            } else {
                                e.body.clone()
                            };
                            Frame::none()
                                .fill(p.code_bg)
                                .inner_margin(Margin::same(6.0))
                                .rounding(Rounding::same(4.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(body).monospace().small().color(p.text));
                                });
                        });
                });
        }
        EntryKind::Text => {
            Frame::none()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .inner_margin(Margin::symmetric(12.0, 10.0))
                .rounding(Rounding::same(p.card_rounding))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    render_assistant_markdown(ui, &e.body, p);
                    if is_last && (turn_usage.total > 0 || total_usage.total > 0) {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("turn: {}", turn_usage.readout()))
                                    .small()
                                    .monospace()
                                    .color(p.muted),
                            );
                            ui.label(
                                RichText::new(format!("· session: {}", total_usage.readout()))
                                    .small()
                                    .monospace()
                                    .color(p.muted),
                            );
                            if let Some(tid) = turn_id {
                                ui.label(RichText::new(short_id(tid)).small().monospace().color(p.muted));
                            }
                        });
                    }
                });
        }
    }
    ui.add_space(6.0);
}
