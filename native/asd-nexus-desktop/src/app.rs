//! The egui desktop application: left rail + six workspace tabs.
//!
//! All rendering goes through [`NexusApp::ui`], which only needs an
//! `egui::Context`, so the full UI can be exercised headlessly in tests
//! without a window or GPU.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use asd_nexus_client::*;
use egui::{Color32, Frame, Margin, RichText, ScrollArea, Stroke, TextEdit};

use crate::backend::{resume_to_spec, Backend, Reply, Request, SavePlanOutcome};
use crate::state::*;
use crate::theme::{self, BadgeStyle};
use crate::timeline::{Entry, EntryKind, Timeline};

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

#[derive(Default)]
pub struct ChatState {
    pub harness_idx: usize,
    pub picker: ModelPicker,
    pub timeline: Timeline,
    pub composer: String,
    pub turn_seq: Option<u64>,
    pub handle: Option<TurnStreamHandle>,
    pub cancel_requested: bool,
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
    pub tab: Tab,
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
            tab: Tab::Chat,
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
        theme::apply(ctx);
        let loaded = crate::fonts::configure_fonts(ctx);
        self.font_status = crate::fonts::describe(loaded.as_deref());
        self.backend.set_repaint(ctx.clone());
        self.repaint_wired = true;
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
        self.refresh_project_data();
    }

    fn refresh_project_data(&mut self) {
        let root = self.project_root.clone();
        self.request(Request::Sessions { project_root: root.clone() });
        self.refresh_board();
        self.load_plan(false);
        self.request(Request::Routing { project_root: root });
        self.refresh_subagents();
    }

    /// Switch the active project (rail selection or OS folder picker).
    pub fn switch_project(&mut self, root: String) {
        let root = clean_root(Path::new(&root));
        if root == self.project_root && !self.sessions.is_empty() {
            return;
        }
        self.project_root = root.clone();
        self.sessions.clear();
        self.selected_session = None;
        self.active_session_id = None;
        if !self.chat.is_streaming() {
            self.chat.timeline.clear();
        }
        self.board = BoardState::default();
        self.plan = PlanEditor::default();
        self.diff = DiffState::default();
        self.subagents = SubagentsState::default();
        self.routing = RoutingEditor::default();
        self.terminal.resume = None;
        if !self.projects.iter().any(|p| p.project_root == root) {
            self.projects.insert(
                0,
                ProjectSummaryItem { project_root: root.clone(), project_hash: String::new(), session_count: None, last_active_at: None },
            );
        }
        self.set_status(format!("project: {root}"));
        self.request(Request::SelectProject(root));
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

    pub fn select_session(&mut self, idx: usize) {
        let Some(s) = self.sessions.get(idx).cloned() else { return };
        self.selected_session = Some((s.harness.clone(), s.harness_session_id.clone()));
        self.active_session_id = s.asd_session_id.clone();
        if let Some(i) = HARNESSES.iter().position(|h| *h == s.harness) {
            self.chat.harness_idx = i;
            self.chat.picker.on_harness_changed(&s.harness);
        }
        if s.model.is_some() {
            self.chat.picker.selected = s.model.clone();
        }
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
        };
        self.request(Request::CreateSession(req));
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

    pub fn send_prompt(&mut self) {
        let prompt = self.chat.composer.trim().to_string();
        if prompt.is_empty() || self.chat.is_streaming() {
            return;
        }
        let req = TurnStreamRequest {
            project_root: self.root_opt(),
            session_id: self.active_session_id.clone(),
            harness: self.chat.harness().to_string(),
            model: self.chat.picker.selected.clone(),
            prompt: prompt.clone(),
            source: Some("desktop".into()),
            streaming: Some(true),
        };
        match self.backend.submit(Request::StartTurn(req)) {
            Ok(seq) => {
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
                    if self.active_session_id.is_none() {
                        self.active_session_id = s.active_session_id;
                    }
                }
                Err(e) => self.set_status(format!("sessions: {e}")),
            },
            Reply::CreateSession(r) => match r {
                Ok(c) if c.success => {
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
                    let h = self.chat.harness();
                    self.chat.picker.on_harness_changed(h);
                }
                Err(e) => self.set_status(format!("models: {e}")),
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
                        self.set_status(format!("turn failed: {e}"));
                    }
                }
            }
            Reply::Stream { seq, msg } => {
                if self.chat.turn_seq != Some(seq) || self.chat.cancel_requested {
                    return;
                }
                self.chat.timeline.apply(&msg);
                if !self.chat.timeline.streaming {
                    self.chat.turn_seq = None;
                    self.chat.handle = None;
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
        }
    }

    // ---------------------------------------------------------------- frame

    /// Render one frame. Usable headlessly with `egui::Context::run`.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.repaint_wired {
            self.backend.set_repaint(ctx.clone());
            self.repaint_wired = true;
        }
        self.frames += 1;
        for r in self.backend.poll() {
            self.handle_reply(r);
        }
        self.shortcuts(ctx);

        // Live subagent polling while the panel is open.
        if self.tab == Tab::Subagents && self.subagents.auto_refresh && self.backend.is_connected() {
            let due = self.subagents.last_poll.map(|t| t.elapsed() > Duration::from_millis(1500)).unwrap_or(true);
            if due && self.backend.in_flight < 4 {
                self.refresh_subagents();
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        egui::TopBottomPanel::top("top").frame(panel_frame(theme::BG)).show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").frame(panel_frame(theme::BG)).show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("rail")
            .resizable(true)
            .default_width(260.0)
            .frame(panel_frame(theme::PANEL))
            .show(ctx, |ui| self.rail(ui));
        egui::CentralPanel::default().frame(panel_frame(theme::BG)).show(ctx, |ui| match self.tab {
            Tab::Chat => self.chat_tab(ui),
            Tab::Board => self.board_tab(ui),
            Tab::Diff => self.diff_tab(ui),
            Tab::Subagents => self.subagents_tab(ui),
            Tab::Routing => self.routing_tab(ui),
            Tab::Terminal => self.terminal_tab(ui),
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let keys = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
        ];
        let mut switch = None;
        ctx.input_mut(|i| {
            for (idx, k) in keys.iter().enumerate() {
                if i.consume_key(egui::Modifiers::COMMAND, *k) {
                    switch = Tab::from_index(idx);
                }
            }
        });
        if let Some(t) = switch {
            self.tab = t;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ASD NEXUS").strong().size(16.0).color(theme::WHITE));
            let (txt, st) = match &self.conn {
                ConnState::Connected(_) => ("CONNECTED", theme::status_badge("done")),
                ConnState::Connecting => ("CONNECTING", theme::status_badge("running")),
                ConnState::Failed(_) => ("OFFLINE", theme::status_badge("failed")),
                ConnState::Offline => ("OFFLINE", theme::status_badge("pending")),
            };
            badge(ui, txt, st);
            ui.separator();
            for t in Tab::ALL {
                let sel = self.tab == t;
                let text = if sel {
                    RichText::new(t.label()).strong().color(theme::WHITE)
                } else {
                    RichText::new(t.label()).color(theme::DIM)
                };
                if ui.selectable_label(sel, text).clicked() {
                    self.tab = t;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.backend.in_flight > 0 || self.chat.is_streaming() {
                    ui.spinner();
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).small().color(theme::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let ConnState::Connected(u) = &self.conn {
                    ui.label(RichText::new(u).small().monospace().color(theme::MUTED));
                }
                if !self.font_status.is_empty() {
                    ui.label(RichText::new(&self.font_status).small().color(theme::MUTED));
                }
                ui.label(RichText::new(format!("jobs {}", self.backend.in_flight)).small().color(theme::MUTED));
            });
        });
    }

    // ---------------------------------------------------------------- rail

    fn rail(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "PROJECTS");
        ui.label(RichText::new(&self.project_root).small().monospace().color(theme::TEXT_HI));
        ui.horizontal(|ui| {
            if ui.button("Open Folder…").on_hover_text("Native OS folder picker").clicked() {
                self.pick_folder();
            }
            if ui.button("Refresh").clicked() {
                self.request(Request::Projects { force: true });
                self.request(Request::Sessions { project_root: self.project_root.clone() });
            }
        });
        let mut switch_to = None;
        ScrollArea::vertical().id_salt("projects").max_height(180.0).show(ui, |ui| {
            for p in &self.projects {
                let sel = p.project_root == self.project_root;
                let label = match p.session_count {
                    Some(n) => format!("{}  ·  {n}", basename(&p.project_root)),
                    None => basename(&p.project_root),
                };
                if ui.selectable_label(sel, label).on_hover_text(&p.project_root).clicked() && !sel {
                    switch_to = Some(p.project_root.clone());
                }
            }
        });
        if let Some(r) = switch_to {
            self.switch_project(r);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            section_title(ui, "SESSIONS");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+ New").on_hover_text("Create a new ASD session").clicked() {
                    self.create_session();
                }
            });
        });
        if let Some(id) = &self.active_session_id {
            ui.label(RichText::new(format!("ASD {}", short_id(id))).small().monospace().color(theme::DIM));
        }
        if let Some(s) = self.selected_session_item() {
            if s.asd_session_id.is_none() && ui.button("Promote to ASD").clicked() {
                self.promote_selected();
            }
        }
        let mut pick = None;
        ScrollArea::vertical().id_salt("sessions").show(ui, |ui| {
            if self.sessions.is_empty() {
                ui.label(RichText::new("no sessions").small().color(theme::MUTED));
            }
            for (i, s) in self.sessions.iter().enumerate() {
                let sel = self.selected_session.as_ref() == Some(&(s.harness.clone(), s.harness_session_id.clone()));
                let title = s
                    .summary
                    .as_deref()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.chars().take(48).collect::<String>())
                    .unwrap_or_else(|| short_id(&s.harness_session_id));
                ui.horizontal(|ui| {
                    if s.is_promoted_to_asd || s.asd_session_id.is_some() {
                        badge(ui, "ASD", theme::status_badge("done"));
                    }
                    let resp = ui.selectable_label(sel, RichText::new(title).small());
                    let resp = resp.on_hover_text(format!(
                        "{} · {}\nmodel: {}\nphase: {}",
                        s.harness,
                        s.harness_session_id,
                        s.model.as_deref().unwrap_or("-"),
                        s.asd_phase.as_deref().unwrap_or("-")
                    ));
                    if resp.clicked() {
                        pick = Some(i);
                    }
                });
                ui.label(RichText::new(&s.harness).small().color(theme::MUTED));
            }
        });
        if let Some(i) = pick {
            self.select_session(i);
        }
    }

    // ---------------------------------------------------------------- chat

    fn chat_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Harness").color(theme::DIM));
            let before = self.chat.harness_idx;
            egui::ComboBox::from_id_salt("harness")
                .selected_text(self.chat.harness())
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (i, h) in HARNESSES.iter().enumerate() {
                        ui.selectable_value(&mut self.chat.harness_idx, i, *h);
                    }
                });
            if before != self.chat.harness_idx {
                let h = self.chat.harness();
                self.chat.picker.on_harness_changed(h);
            }
            ui.label(RichText::new("Model").color(theme::DIM));
            let harness = self.chat.harness();
            let options: Vec<String> = self.chat.picker.filtered(harness).into_iter().map(String::from).collect();
            let total = self.chat.picker.models.get(harness).map(Vec::len).unwrap_or(0);
            let shown = self.chat.picker.selected.clone().unwrap_or_else(|| "(default)".into());
            egui::ComboBox::from_id_salt("model").selected_text(shown).width(260.0).show_ui(ui, |ui| {
                ui.add(TextEdit::singleline(&mut self.chat.picker.filter).hint_text("search models…"));
                ui.label(RichText::new(format!("{} / {total}", options.len())).small().color(theme::MUTED));
                ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    ui.selectable_value(&mut self.chat.picker.selected, None, "(default)");
                    for m in options {
                        ui.selectable_value(&mut self.chat.picker.selected, Some(m.clone()), m);
                    }
                });
            });
            if ui.button("Refresh models").clicked() {
                self.request(Request::Models { refresh: true });
            }
        });
        ui.horizontal(|ui| {
            let tl = &self.chat.timeline;
            ui.label(RichText::new(format!("turn: {}", tl.turn_usage.readout())).small().monospace().color(theme::DIM));
            ui.separator();
            ui.label(RichText::new(format!("session: {}", tl.total_usage.readout())).small().monospace().color(theme::DIM));
            if let Some(t) = &tl.turn_id {
                ui.separator();
                ui.label(RichText::new(short_id(t)).small().monospace().color(theme::MUTED));
            }
        });
        ui.separator();

        let composer_h = 110.0;
        let avail = (ui.available_height() - composer_h).max(80.0);
        ScrollArea::vertical()
            .id_salt("timeline")
            .max_height(avail)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.chat.timeline.entries.is_empty() {
                    ui.label(RichText::new("No messages yet. Pick a session or start typing.").color(theme::MUTED));
                }
                for e in &self.chat.timeline.entries {
                    entry_ui(ui, e);
                }
                if self.chat.timeline.streaming {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("streaming…").small().color(theme::DIM));
                    });
                }
            });
        ui.separator();
        let streaming = self.chat.is_streaming();
        let mut send = false;
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 100.0).max(100.0);
            let resp = ui.add_sized(
                [w, 80.0],
                TextEdit::multiline(&mut self.chat.composer)
                    .hint_text("Message (Ctrl+Enter to send)")
                    .desired_rows(4),
            );
            if resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                send = true;
            }
            ui.vertical(|ui| {
                if ui.add_enabled(!streaming, egui::Button::new("Send").min_size(egui::vec2(84.0, 36.0))).clicked() {
                    send = true;
                }
                if ui.add_enabled(streaming, egui::Button::new("Cancel").min_size(egui::vec2(84.0, 36.0))).clicked() {
                    self.cancel_turn();
                }
            });
        });
        if send {
            self.send_prompt();
        }
    }

    // ---------------------------------------------------------------- board

    fn board_tab(&mut self, ui: &mut egui::Ui) {
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
                .color(theme::DIM));
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
            ui.label(RichText::new(r).small().color(theme::TEXT_HI));
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
                ui.label(RichText::new(format!("{} · {}", KANBAN_COLUMNS[i], col.len())).strong().color(theme::TEXT_HI));
                ScrollArea::vertical().id_salt(("kanban", i)).max_height(220.0).show(ui, |ui| {
                    for t in col {
                        let sel = selected.as_deref() == Some(t.id.as_str());
                        Frame::none()
                            .fill(if sel { theme::RAISED } else { theme::SURFACE })
                            .stroke(Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { theme::WHITE } else { theme::BORDER }))
                            .inner_margin(Margin::same(6.0))
                            .rounding(2.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(sel, RichText::new(&t.id).monospace().strong()).clicked() {
                                        select = Some(t.id.clone());
                                    }
                                    badge(ui, &t.status, theme::status_badge(&t.status));
                                });
                                ui.label(RichText::new(&t.subject).small());
                                let meta = [t.assignee_role.clone(), t.difficulty.clone(), t.turn.map(|n| format!("turn {n}"))]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                if !meta.is_empty() {
                                    ui.label(RichText::new(meta).small().color(theme::MUTED));
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
            self.tab = Tab::Diff;
            self.load_diff();
        }

        ui.separator();
        self.plan_editor(ui);
    }

    fn plan_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("PLAN.md").strong().color(theme::WHITE));
            ui.label(RichText::new(if self.plan.path.is_empty() { "-" } else { &self.plan.path }).small().monospace().color(theme::DIM));
            if self.plan.is_dirty() {
                badge(ui, "UNSAVED", theme::status_badge("running"));
            }
            if !self.plan.exists && self.plan.loaded {
                badge(ui, "NEW FILE", theme::status_badge("pending"));
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
                    .color(theme::MUTED),
                );
            });
        });
        if let Some(c) = self.plan.conflict.clone() {
            let mut take = false;
            let mut keep = false;
            Frame::none()
                .fill(theme::BG)
                .stroke(Stroke::new(2.0_f32, theme::WHITE))
                .inner_margin(Margin::same(8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        badge(ui, "CONFLICT", theme::status_badge("failed"));
                        ui.label(RichText::new(&c.reason).color(theme::WHITE));
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("Task").color(theme::DIM));
            ui.add(TextEdit::singleline(&mut self.diff.task_id).desired_width(120.0).hint_text("T1.2"));
            ui.label(RichText::new("Turn").color(theme::DIM));
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
                .color(theme::DIM),
            );
            if let Some(e) = &d.error {
                badge(ui, &format!("ERROR {e}"), theme::status_badge("failed"));
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
            ui.label(RichText::new(r).small().color(theme::TEXT_HI));
        }
        ui.separator();
        let Some(d) = &self.diff.resp else {
            ui.label(RichText::new("No diff loaded.").color(theme::MUTED));
            return;
        };
        egui::Grid::new("diff_files").striped(true).num_columns(4).show(ui, |ui| {
            for f in &d.files {
                ui.label(RichText::new(f.status.as_deref().unwrap_or("M")).monospace().strong());
                ui.label(RichText::new(&f.path).monospace());
                ui.label(RichText::new(format!("+{}", f.additions.unwrap_or(0))).monospace().color(theme::WHITE));
                ui.label(RichText::new(format!("-{}", f.deletions.unwrap_or(0))).monospace().color(theme::MUTED).strikethrough());
                ui.end_row();
            }
        });
        ui.separator();
        let text = d.diff_text.clone().unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        ScrollArea::both().id_salt("diff_text").auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
            for line in &lines[range] {
                let st = theme::diff_style(theme::classify_diff_line(line));
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
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_subagents();
            }
            ui.checkbox(&mut self.subagents.auto_refresh, "live (1.5s)");
            ui.label(RichText::new(format!("{} subagents", self.subagents.list.len())).small().color(theme::DIM));
        });
        ui.separator();
        let mut pick = None;
        ScrollArea::vertical().id_salt("subagents").max_height(220.0).show(ui, |ui| {
            egui::Grid::new("subagent_grid").striped(true).num_columns(7).show(ui, |ui| {
                for h in ["sub-session", "status", "role", "harness", "model", "task", "exit"] {
                    ui.label(RichText::new(h).small().strong().color(theme::DIM));
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
                    badge(ui, &s.status, theme::status_badge(&s.status));
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
            ui.label(RichText::new(r).small().color(theme::TEXT_HI));
        }
        ui.separator();
        ui.label(RichText::new("LOG TAIL").small().strong().color(theme::DIM));
        Frame::none().fill(theme::BG).stroke(Stroke::new(1.0_f32, theme::BORDER)).inner_margin(Margin::same(6.0)).show(ui, |ui| {
            ScrollArea::both().id_salt("logtail").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                if self.subagents.logs.is_empty() {
                    ui.label(RichText::new("select a subagent to tail its output.log").small().color(theme::MUTED));
                }
                for l in &self.subagents.logs {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        });
    }

    // ---------------------------------------------------------------- routing

    fn routing_tab(&mut self, ui: &mut egui::Ui) {
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
                badge(ui, "UNSAVED", theme::status_badge("running"));
            }
        });
        if let Some(e) = &self.routing.error {
            badge(ui, e, theme::status_badge("failed"));
        }
        ui.separator();
        ScrollArea::vertical().id_salt("routing").auto_shrink([false, false]).show(ui, |ui| {
            for (i, key) in ROUTING_SECTIONS.iter().enumerate() {
                egui::CollapsingHeader::new(RichText::new(*key).strong().color(theme::WHITE))
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
        let session = self.selected_session_item().cloned();
        let sid = self.session_for_actions();
        egui::Grid::new("term_info").num_columns(2).show(ui, |ui| {
            ui.label(RichText::new("ASD session").color(theme::DIM));
            ui.label(RichText::new(sid.clone().unwrap_or_else(|| "-".into())).monospace());
            ui.end_row();
            ui.label(RichText::new("harness").color(theme::DIM));
            ui.label(session.as_ref().map(|s| s.harness.clone()).unwrap_or_else(|| self.chat.harness().into()));
            ui.end_row();
            ui.label(RichText::new("harness session").color(theme::DIM));
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
            ui.label(RichText::new("INTERACTIVE RESUME COMMAND").small().strong().color(theme::DIM));
            let mut cli = r.cli_string.as_str();
            ui.add(TextEdit::multiline(&mut cli).code_editor().desired_rows(2).desired_width(f32::INFINITY));
            ui.label(RichText::new(format!("cwd: {}", r.cwd)).small().monospace().color(theme::MUTED));
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
            ui.label(RichText::new("Select an ASD session and fetch its resume command.").color(theme::MUTED));
        }
        if let Some(r) = &self.terminal.last_result {
            ui.label(RichText::new(r).small().color(theme::TEXT_HI));
        }
        ui.separator();
        ui.label(RichText::new("ACTIVITY").small().strong().color(theme::DIM));
        ScrollArea::vertical().id_salt("activity").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for l in &self.log {
                ui.label(RichText::new(l).small().monospace().color(theme::TEXT));
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
        egui::Rgba::from(theme::BG).to_array()
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

fn panel_frame(fill: Color32) -> Frame {
    Frame::none().fill(fill).inner_margin(Margin::same(8.0)).stroke(Stroke::new(1.0_f32, theme::BORDER))
}

fn section_title(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).small().strong().color(theme::DIM));
}

/// Monochrome pill badge.
pub fn badge(ui: &mut egui::Ui, text: &str, st: BadgeStyle) -> egui::Response {
    Frame::none()
        .fill(st.bg)
        .stroke(st.stroke)
        .inner_margin(Margin::symmetric(5.0, 1.0))
        .rounding(2.0)
        .show(ui, |ui| ui.label(RichText::new(text.to_uppercase()).small().strong().color(st.fg)))
        .response
}

fn entry_ui(ui: &mut egui::Ui, e: &Entry) {
    match e.kind {
        EntryKind::User => {
            Frame::none()
                .fill(theme::RAISED)
                .stroke(Stroke::new(1.0_f32, theme::BORDER_HI))
                .inner_margin(Margin::same(8.0))
                .rounding(2.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("YOU").small().strong().color(theme::DIM));
                    ui.label(RichText::new(&e.body).color(theme::WHITE));
                });
        }
        EntryKind::Error => {
            Frame::none()
                .fill(theme::BG)
                .stroke(Stroke::new(2.0_f32, theme::WHITE))
                .inner_margin(Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        badge(ui, "ERROR", theme::status_badge("failed"));
                        if !e.title.is_empty() {
                            ui.label(RichText::new(&e.title).small().color(theme::DIM));
                        }
                    });
                    ui.label(RichText::new(&e.body).color(theme::WHITE));
                });
        }
        EntryKind::System => {
            ui.label(RichText::new(format!("— {} —", e.body)).small().italics().color(theme::MUTED));
        }
        kind => {
            let id = ui.make_persistent_id(("entry", e.id));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, kind.default_open());
            let stroke = if e.is_error { Stroke::new(2.0_f32, theme::WHITE) } else { Stroke::new(1.0_f32, theme::BORDER) };
            Frame::none().fill(theme::SURFACE).stroke(stroke).inner_margin(Margin::same(6.0)).rounding(2.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                state
                    .show_header(ui, |ui| {
                        ui.label(RichText::new(kind.label()).small().strong().color(theme::DIM));
                        if !e.title.is_empty() {
                            ui.label(RichText::new(&e.title).monospace().strong().color(theme::TEXT_HI));
                        }
                        if e.pending {
                            ui.spinner();
                        }
                        if e.is_error {
                            badge(ui, "isError", theme::status_badge("failed"));
                        }
                        if kind == EntryKind::Thinking {
                            ui.label(RichText::new(format!("{} chars", e.body.chars().count())).small().color(theme::MUTED));
                        }
                    })
                    .body(|ui| match kind {
                        EntryKind::Thinking => {
                            ui.label(RichText::new(&e.body).italics().color(theme::DIM));
                        }
                        EntryKind::ToolUse | EntryKind::ToolResult => {
                            let body = if e.body.chars().count() > 8000 {
                                e.body.chars().take(8000).collect::<String>() + "\n… (truncated)"
                            } else {
                                e.body.clone()
                            };
                            ui.label(RichText::new(body).monospace().small().color(theme::TEXT));
                        }
                        _ => {
                            ui.label(RichText::new(&e.body).color(theme::TEXT_HI));
                        }
                    });
            });
        }
    }
    ui.add_space(4.0);
}
