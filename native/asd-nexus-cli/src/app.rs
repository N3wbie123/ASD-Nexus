//! Interactive TUI application state and key handling.
//!
//! Key handling is pure: it mutates UI state and queues [`Action`]s, which are
//! executed against the sidecar by [`crate::exec`]. This keeps every key path
//! testable without a server.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use asd_nexus_client::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::textarea::TextArea;
use crate::timeline::Timeline;

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
    RefreshModels { force: bool },
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

pub struct App {
    pub project_root: String,
    pub harness: String,
    pub model: Option<String>,
    pub cli_session: Option<String>,
    pub server_label: String,

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

    pub models: std::collections::HashMap<String, Vec<String>>,
    pub picker_harness: usize,
    pub picker_query: String,
    pub picker_sel: usize,

    pub timeline: Timeline,
    pub timeline_sel: usize,
    pub input: String,
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
        Self {
            project_root: project_root.into(),
            harness,
            model,
            cli_session: None,
            server_label: String::new(),
            focus: Focus::Sessions,
            overlay: None,
            prompt: None,
            confirm: None,
            pending: vec![Action::LoadInitial],
            status: "Ready. F1 = help".into(),
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
            models: Default::default(),
            picker_harness,
            picker_query: String::new(),
            picker_sel: 0,
            timeline: Timeline::default(),
            timeline_sel: 0,
            input: String::new(),
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

    pub fn is_streaming(&self) -> bool {
        self.stream.is_some() || self.timeline.streaming
    }

    /// ASD session used for board / subagents / turns.
    pub fn active_session_id(&self) -> Option<String> {
        self.current_asd
            .clone()
            .or_else(|| self.cli_session.clone())
            .or_else(|| self.foreground_id.clone())
    }

    // ---------------------------------------------------------------- sessions

    pub fn filtered_sessions(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| match self.harness_filter {
                Some(h) => s.harness == HARNESSES[h],
                None => true,
            })
            .filter(|(_, s)| {
                q.is_empty()
                    || [
                        Some(&s.harness),
                        Some(&s.harness_session_id),
                        s.summary.as_ref(),
                        s.model.as_ref(),
                        s.asd_session_id.as_ref(),
                        s.asd_phase.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|f| f.to_lowercase().contains(&q))
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
            self.timeline.apply(m);
        }
        if closed || !self.timeline.streaming {
            self.timeline.end_turn(Some("stream closed"));
            self.stream = None;
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

    fn open_overlay(&mut self, o: Overlay) {
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
            Overlay::Help => {}
        }
    }

    pub fn board_tasks(&self) -> &[BoardTaskItem] {
        self.board.as_ref().map(|b| b.tasks.as_slice()).unwrap_or(&[])
    }

    // -------------------------------------------------------------- key input

    pub fn handle_key(&mut self, k: KeyEvent) {
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

        if k.code == KeyCode::Tab || k.code == KeyCode::BackTab {
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
                    PromptPurpose::NewSessionTitle => self.push(Action::CreateSession { title: opt }),
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
                self.harness = HARNESSES[(i + 1) % HARNESSES.len()].to_string();
                self.model = None;
                self.set_status(format!("Harness: {}", self.harness));
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
            return;
        }
        match k.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.timeline.toggle(self.timeline_sel),
            KeyCode::Char('i') => self.focus = Focus::Input,
            _ => {}
        }
    }

    fn handle_input_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return;
                }
                if self.is_streaming() {
                    self.set_status("A turn is already streaming (Ctrl+X to cancel)");
                    return;
                }
                self.input.clear();
                self.push(Action::SendPrompt(text));
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Esc => self.focus = Focus::Timeline,
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
    }

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
            Overlay::Board => self.handle_board_key(k),
            Overlay::Subagents => self.handle_subagents_key(k),
            Overlay::Diff => self.handle_diff_key(k),
            Overlay::Routing => self.handle_routing_key(k),
            Overlay::Terminal => self.handle_terminal_key(k),
        }
    }

    fn handle_picker_key(&mut self, k: KeyEvent) {
        let n = self.picker_models().len();
        match k.code {
            KeyCode::Up => self.picker_sel = self.picker_sel.saturating_sub(1),
            KeyCode::Down => self.picker_sel = (self.picker_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Tab | KeyCode::Right => {
                self.picker_harness = (self.picker_harness + 1) % HARNESSES.len();
                self.picker_sel = 0;
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.picker_harness = (self.picker_harness + HARNESSES.len() - 1) % HARNESSES.len();
                self.picker_sel = 0;
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
                self.harness = HARNESSES[self.picker_harness].to_string();
                self.model = models.get(self.picker_sel).filter(|_| self.picker_sel > 0).cloned();
                self.overlay = None;
                self.set_status(format!(
                    "Harness {} · model {}",
                    self.harness,
                    self.model.as_deref().unwrap_or("(default)")
                ));
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
