//! Background job runner: every blocking `NexusClient` call runs on a worker
//! thread and reports back through an MPSC channel polled by the UI each frame.
//! The UI thread never blocks on HTTP.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use asd_nexus_client::*;

/// A request issued by the UI.
#[derive(Debug, Clone)]
pub enum Request {
    Health,
    Projects { force: bool },
    SelectProject(String),
    Sessions { project_root: String },
    CreateSession(CreateSessionRequest),
    PromoteSession(PromoteSessionRequest),
    Transcript(GetTranscriptRequest),
    Models { refresh: bool },
    HarnessCommands(GetHarnessCommandsRequest),
    FsList(FsListRequest),
    Board(GetBoardRequest),
    Plan { project_root: Option<String>, session_id: Option<String> },
    /// Pre-flight GET then save if sha/mtime unchanged.
    SavePlan { req: SavePlanRequest, expected_sha256: Option<String>, expected_mtime_ms: Option<f64> },
    SyncPlan(PlanSyncRequest),
    Dispatch(DispatchRequest),
    Debate(DebateRequest),
    Diff(WorktreeDiffRequest),
    Approve(ApproveTaskRequest),
    Reject(RejectTaskRequest),
    Subagents(GetSubagentsRequest),
    SubagentLogs(SubagentLogTailRequest),
    SubagentAction(SubagentActionRequest),
    Routing { project_root: String },
    SaveRouting(SaveRoutingRequest),
    ResumeCommand { session_id: String, project_root: Option<String> },
    TerminalOpen { req: TerminalOpenRequest, kind: Option<TerminalKind> },
    LaunchLocal { spec: TerminalOpenResponse, kind: Option<TerminalKind> },
    StartTurn(TurnStreamRequest),
    CancelTurn(TurnStreamHandle),
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Request::Health => "health",
            Request::Projects { .. } => "projects",
            Request::SelectProject(_) => "select project",
            Request::Sessions { .. } => "sessions",
            Request::CreateSession(_) => "create session",
            Request::PromoteSession(_) => "promote session",
            Request::Transcript(_) => "transcript",
            Request::Models { .. } => "models",
            Request::HarnessCommands(_) => "harness commands",
            Request::FsList(_) => "fs list",
            Request::Board(_) => "board",
            Request::Plan { .. } => "plan",
            Request::SavePlan { .. } => "save plan",
            Request::SyncPlan(_) => "sync plan",
            Request::Dispatch(_) => "dispatch",
            Request::Debate(_) => "debate",
            Request::Diff(_) => "worktree diff",
            Request::Approve(_) => "approve",
            Request::Reject(_) => "reject",
            Request::Subagents(_) => "subagents",
            Request::SubagentLogs(_) => "subagent logs",
            Request::SubagentAction(_) => "subagent action",
            Request::Routing { .. } => "routing",
            Request::SaveRouting(_) => "save routing",
            Request::ResumeCommand { .. } => "resume command",
            Request::TerminalOpen { .. } => "terminal open",
            Request::LaunchLocal { .. } => "launch terminal",
            Request::StartTurn(_) => "turn",
            Request::CancelTurn(_) => "cancel turn",
        }
    }
}

pub type R<T> = std::result::Result<T, String>;

/// Outcome of the PLAN save flow.
#[derive(Debug, Clone)]
pub enum SavePlanOutcome {
    Saved(SavePlanResponse),
    /// Pre-flight detected drift; carries the fresh on-disk plan.
    Drift(GetPlanResponse),
}

/// A reply delivered to the UI.
#[derive(Debug)]
pub enum Reply {
    Connected(R<(String, Option<String>)>),
    Health(R<GetHealthResponse>),
    Projects(R<ListProjectsResponse>),
    SelectProject(R<SelectProjectResponse>),
    Sessions(R<GetSessionsResponse>),
    CreateSession(R<CreateSessionResponse>),
    PromoteSession(R<PromoteSessionResponse>),
    Transcript(R<GetTranscriptResponse>),
    Models(R<GetModelsResponse>),
    HarnessCommands(R<GetHarnessCommandsResponse>),
    FsList(R<FsListResponse>),
    Board(R<GetBoardResponse>),
    Plan(R<GetPlanResponse>),
    SavePlan(R<SavePlanOutcome>),
    SyncPlan(R<PlanSyncResponse>),
    Dispatch(R<DispatchResponse>),
    Debate(R<DebateResponse>),
    Diff(R<WorktreeDiffResponse>),
    Approve(R<ApproveTaskResponse>),
    Reject(R<RejectTaskResponse>),
    Subagents(R<GetSubagentsResponse>),
    SubagentLogs(R<SubagentLogTailResponse>),
    SubagentAction(R<SubagentActionResponse>),
    Routing(R<GetRoutingResponse>),
    SaveRouting(R<SaveRoutingResponse>),
    ResumeCommand(R<GetResumeCommandResponse>),
    Terminal(R<String>),
    TurnStarted { seq: u64, handle: R<TurnStreamHandle> },
    Stream { seq: u64, msg: StreamMessage },
    TurnCancelled(R<TurnCancelResponse>),
}

fn e<T>(r: asd_nexus_client::Result<T>) -> R<T> {
    r.map_err(|e| e.to_string())
}

/// Turn a resume command into a terminal spec for `launch_terminal_spec`.
pub fn resume_to_spec(r: &GetResumeCommandResponse) -> TerminalOpenResponse {
    TerminalOpenResponse {
        success: r.success,
        command: Some(r.command.clone()),
        args: Some(r.args.clone()),
        cli_string: Some(r.cli_string.clone()),
        cwd: Some(r.cwd.clone()),
        launched: Some(false),
        error: r.error.clone(),
    }
}

fn launch(spec: &TerminalOpenResponse, kind: Option<TerminalKind>) -> R<String> {
    match launch_terminal_spec(spec, kind) {
        Ok(child) => Ok(format!(
            "launched terminal (pid {}): {}",
            child.id(),
            spec.cli_string.clone().or_else(|| spec.command.clone()).unwrap_or_default()
        )),
        Err(err) => Err(err.to_string()),
    }
}

/// Execute a blocking request (runs on a worker thread).
pub fn execute(c: &NexusClient, req: Request, tx: &Sender<Reply>, seq: u64) -> Option<Reply> {
    Some(match req {
        Request::Health => Reply::Health(e(c.health())),
        Request::Projects { force } => Reply::Projects(e(c.projects(&ListProjectsRequest {
            skip_disk_check: None,
            force_refresh: Some(force),
        }))),
        Request::SelectProject(root) => Reply::SelectProject(e(c.select_project(&SelectProjectRequest {
            project_root: root,
            initialize: Some(true),
        }))),
        Request::Sessions { project_root } => Reply::Sessions(e(c.sessions(&GetSessionsRequest {
            project_root: Some(project_root),
            limit: Some(200),
            sync_local_transcripts: Some(true),
        }))),
        Request::CreateSession(r) => Reply::CreateSession(e(c.create_session(&r))),
        Request::PromoteSession(r) => Reply::PromoteSession(e(c.promote_session(&r))),
        Request::Transcript(r) => Reply::Transcript(e(c.transcript(&r))),
        Request::Models { refresh } => Reply::Models(e(if refresh {
            c.refresh_models()
        } else {
            c.models(&GetModelsRequest::default())
        })),
        Request::HarnessCommands(r) => Reply::HarnessCommands(e(c.harness_commands(&r))),
        Request::FsList(r) => Reply::FsList(e(c.fs_list(&r))),
        Request::Board(r) => Reply::Board(e(c.board(&r))),
        Request::Plan { project_root, session_id } => {
            Reply::Plan(e(c.get_plan(project_root.as_deref(), session_id.as_deref())))
        }
        Request::SavePlan { req, expected_sha256, expected_mtime_ms } => {
            let fresh = c.get_plan(req.project_root.as_deref(), req.session_id.as_deref());
            Reply::SavePlan(match fresh {
                Err(err) => Err(err.to_string()),
                Ok(fresh) => {
                    if plan_drifted(&fresh, expected_sha256.as_deref(), expected_mtime_ms) {
                        Ok(SavePlanOutcome::Drift(fresh))
                    } else {
                        e(c.save_plan(&req)).map(SavePlanOutcome::Saved)
                    }
                }
            })
        }
        Request::SyncPlan(r) => Reply::SyncPlan(e(c.sync_plan(&r))),
        Request::Dispatch(r) => Reply::Dispatch(e(c.dispatch(&r))),
        Request::Debate(r) => Reply::Debate(e(c.debate(&r))),
        Request::Diff(r) => Reply::Diff(e(c.worktree_diff(&r))),
        Request::Approve(r) => Reply::Approve(e(c.approve_task(&r))),
        Request::Reject(r) => Reply::Reject(e(c.reject_task(&r))),
        Request::Subagents(r) => Reply::Subagents(e(c.subagents(&r))),
        Request::SubagentLogs(r) => Reply::SubagentLogs(e(c.subagent_logs(&r))),
        Request::SubagentAction(r) => Reply::SubagentAction(e(c.subagent_action(&r))),
        Request::Routing { project_root } => Reply::Routing(e(c.get_routing(&GetRoutingRequest { project_root }))),
        Request::SaveRouting(r) => Reply::SaveRouting(e(c.save_routing(&r))),
        Request::ResumeCommand { session_id, project_root } => {
            Reply::ResumeCommand(e(c.resume_command(&session_id, project_root.as_deref())))
        }
        Request::TerminalOpen { req, kind } => Reply::Terminal(match c.terminal_open(&req) {
            Err(err) => Err(err.to_string()),
            Ok(spec) if !spec.success => Err(spec.error.unwrap_or_else(|| "terminal open failed".into())),
            Ok(spec) if spec.launched == Some(true) => {
                Ok(format!("server launched terminal: {}", spec.cli_string.unwrap_or_default()))
            }
            Ok(spec) => launch(&spec, kind),
        }),
        Request::LaunchLocal { spec, kind } => Reply::Terminal(launch(&spec, kind)),
        Request::StartTurn(r) => match c.stream_turn(&r) {
            Err(err) => Reply::TurnStarted { seq, handle: Err(err.to_string()) },
            Ok((handle, rx)) => {
                if tx.send(Reply::TurnStarted { seq, handle: Ok(handle) }).is_err() {
                    return None;
                }
                for msg in rx {
                    let done = msg == StreamMessage::Done;
                    if tx.send(Reply::Stream { seq, msg }).is_err() {
                        return None;
                    }
                    if done {
                        break;
                    }
                }
                return None;
            }
        },
        Request::CancelTurn(h) => Reply::TurnCancelled(e(h.cancel())),
    })
}

/// expectedSha256 / expectedMtimeMs drift check.
pub fn plan_drifted(fresh: &GetPlanResponse, expected_sha256: Option<&str>, expected_mtime_ms: Option<f64>) -> bool {
    if let (Some(exp), Some(cur)) = (expected_sha256, fresh.sha256.as_deref()) {
        return exp != cur;
    }
    if let (Some(exp), Some(cur)) = (expected_mtime_ms, fresh.mtime_ms) {
        return (exp - cur).abs() > 0.5;
    }
    false
}

/// UI-side handle to the job runner.
pub struct Backend {
    client: Option<NexusClient>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
    repaint: Option<egui::Context>,
    pub in_flight: usize,
    pub submitted: Vec<Request>,
    seq: u64,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Backend {
    pub fn new(client: Option<NexusClient>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { client, tx, rx, repaint: None, in_flight: 0, submitted: Vec::new(), seq: 0 }
    }

    pub fn set_repaint(&mut self, ctx: egui::Context) {
        self.repaint = Some(ctx);
    }

    pub fn set_client(&mut self, client: Option<NexusClient>) {
        self.client = client;
    }

    pub fn client(&self) -> Option<&NexusClient> {
        self.client.as_ref()
    }

    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    /// A sender other threads (e.g. sidecar startup) can use to post replies.
    pub fn sender(&self) -> Sender<Reply> {
        self.tx.clone()
    }

    /// Submit a request. Returns the sequence number, or an error if not connected.
    pub fn submit(&mut self, req: Request) -> R<u64> {
        self.submitted.push(req.clone());
        if self.submitted.len() > 256 {
            self.submitted.remove(0);
        }
        let Some(client) = self.client.clone() else {
            return Err(format!("{}: not connected to sidecar", req.name()));
        };
        self.seq += 1;
        let seq = self.seq;
        let tx = self.tx.clone();
        let repaint = self.repaint.clone();
        let streaming = matches!(req, Request::StartTurn(_));
        if !streaming {
            self.in_flight += 1;
        }
        thread::Builder::new()
            .name(format!("nexus-{}", req.name()))
            .spawn(move || {
                let ping = |r: &Option<egui::Context>| {
                    if let Some(ctx) = r {
                        ctx.request_repaint();
                    }
                };
                // Stream replies are forwarded inside `execute`; wake the UI per message.
                let (itx, irx) = mpsc::channel::<Reply>();
                let fwd_tx = tx.clone();
                let fwd_repaint = repaint.clone();
                let forwarder = thread::spawn(move || {
                    for r in irx {
                        if fwd_tx.send(r).is_err() {
                            break;
                        }
                        if let Some(ctx) = &fwd_repaint {
                            ctx.request_repaint();
                        }
                    }
                });
                if let Some(reply) = execute(&client, req, &itx, seq) {
                    let _ = itx.send(reply);
                }
                drop(itx);
                let _ = forwarder.join();
                ping(&repaint);
            })
            .map_err(|e| e.to_string())?;
        Ok(seq)
    }

    /// Drain all pending replies.
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut out = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            if !matches!(r, Reply::Stream { .. } | Reply::TurnStarted { .. } | Reply::Connected(_)) {
                self.in_flight = self.in_flight.saturating_sub(1);
            }
            out.push(r);
        }
        out
    }

    /// Inject a reply (tests / sidecar startup).
    pub fn inject(&self, reply: Reply) {
        let _ = self.tx.send(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_without_client_errors() {
        let mut b = Backend::new(None);
        assert!(b.submit(Request::Health).unwrap_err().contains("not connected"));
        assert_eq!(b.in_flight, 0);
        assert_eq!(b.submitted.len(), 1);
    }

    #[test]
    fn unreachable_server_reports_error() {
        // Port 9 (discard) on loopback: connection refused quickly.
        let mut b = Backend::new(Some(NexusClient::new("http://127.0.0.1:9", None)));
        b.submit(Request::Health).unwrap();
        let start = std::time::Instant::now();
        loop {
            let replies = b.poll();
            if let Some(Reply::Health(r)) = replies.into_iter().next() {
                assert!(r.is_err());
                break;
            }
            assert!(start.elapsed().as_secs() < 30, "timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(b.in_flight, 0);
    }

    #[test]
    fn drift_detection() {
        let p = GetPlanResponse {
            session_id: None,
            plan_md_path: "p".into(),
            exists: true,
            content: String::new(),
            mtime_ms: Some(10.0),
            sha256: Some("a".into()),
        };
        assert!(!plan_drifted(&p, Some("a"), Some(1.0)));
        assert!(plan_drifted(&p, Some("b"), Some(10.0)));
        assert!(plan_drifted(&p, None, Some(1.0)));
        assert!(!plan_drifted(&p, None, None));
    }
}
