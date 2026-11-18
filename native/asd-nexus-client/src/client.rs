//! NexusClient: REST and SSE Client for the ASD-Nexus Sidecar Server.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use url::Url;

use crate::error::{NexusError, Result};
use crate::models::*;
use crate::sse::{SseParser, StreamMessage};

/// Strongly-typed client for communicating with the ASD-Nexus sidecar server.
#[derive(Clone, Debug)]
pub struct NexusClient {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl NexusClient {
    /// Create a new client pointing to the given base URL.
    pub fn new<S: Into<String>>(base_url: S, token: Option<String>) -> Self {
        let agent = ureq::builder()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(120))
            .build();

        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn apply_headers<'a>(&self, mut req: ureq::Request) -> ureq::Request {
        if let Some(ref token) = self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        req
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let req = self.apply_headers(self.agent.get(url));
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(NexusError::HttpStatus { status: code, body });
            }
            Err(ureq::Error::Transport(t)) => return Err(NexusError::Http(t.to_string())),
        };

        resp.into_json::<T>().map_err(Into::into)
    }

    fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let req = self.apply_headers(self.agent.post(url));
        let resp = match req.send_json(body) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                // Check if the response contains a valid JSON payload of type T (e.g., 409 Conflict)
                if let Ok(val) = r.into_json::<T>() {
                    return Ok(val);
                }
                let body = format!("Status {code}");
                return Err(NexusError::HttpStatus { status: code, body });
            }
            Err(ureq::Error::Transport(t)) => return Err(NexusError::Http(t.to_string())),
        };

        resp.into_json::<T>().map_err(Into::into)
    }

    fn delete_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let req = self.apply_headers(self.agent.delete(url));
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(NexusError::HttpStatus { status: code, body });
            }
            Err(ureq::Error::Transport(t)) => return Err(NexusError::Http(t.to_string())),
        };

        resp.into_json::<T>().map_err(Into::into)
    }

    // ========================================================================
    // Health
    // ========================================================================

    pub fn health(&self) -> Result<GetHealthResponse> {
        let url = format!("{}/api/health", self.base_url);
        self.get_json(&url)
    }

    // ========================================================================
    // Projects
    // ========================================================================

    pub fn projects(&self, req: &ListProjectsRequest) -> Result<ListProjectsResponse> {
        let mut url = Url::parse(&format!("{}/api/projects", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(skip) = req.skip_disk_check {
                q.append_pair("skipDiskCheck", &skip.to_string());
            }
            if let Some(force) = req.force_refresh {
                q.append_pair("forceRefresh", &force.to_string());
            }
        }
        self.get_json(url.as_str())
    }

    pub fn select_project(&self, req: &SelectProjectRequest) -> Result<SelectProjectResponse> {
        let url = format!("{}/api/projects/select", self.base_url);
        self.post_json(&url, req)
    }

    pub fn resolve_project_paths(
        &self,
        req: &ResolveProjectPathsRequest,
    ) -> Result<ResolveProjectPathsResponse> {
        let mut url = Url::parse(&format!("{}/api/projects/resolve", self.base_url))?;
        url.query_pairs_mut()
            .append_pair("projectRoot", &req.project_root);
        self.get_json(url.as_str())
    }

    // ========================================================================
    // Models & Routing
    // ========================================================================

    pub fn models(&self, req: &GetModelsRequest) -> Result<GetModelsResponse> {
        let mut url = Url::parse(&format!("{}/api/models", self.base_url))?;
        if let Some(force) = req.force_refresh {
            url.query_pairs_mut()
                .append_pair("forceRefresh", &force.to_string());
        }
        self.get_json(url.as_str())
    }

    pub fn refresh_models(&self) -> Result<GetModelsResponse> {
        let url = format!("{}/api/models/refresh", self.base_url);
        self.post_json(&url, &serde_json::json!({}))
    }

    pub fn get_routing(&self, req: &GetRoutingRequest) -> Result<GetRoutingResponse> {
        let mut url = Url::parse(&format!("{}/api/routing", self.base_url))?;
        url.query_pairs_mut()
            .append_pair("projectRoot", &req.project_root);
        self.get_json(url.as_str())
    }

    pub fn save_routing(&self, req: &SaveRoutingRequest) -> Result<SaveRoutingResponse> {
        let url = format!("{}/api/routing", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Sessions
    // ========================================================================

    pub fn sessions(&self, req: &GetSessionsRequest) -> Result<GetSessionsResponse> {
        let mut url = Url::parse(&format!("{}/api/sessions", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(limit) = req.limit {
                q.append_pair("limit", &limit.to_string());
            }
            if let Some(sync) = req.sync_local_transcripts {
                q.append_pair("syncLocalTranscripts", &sync.to_string());
            }
        }
        self.get_json(url.as_str())
    }

    pub fn create_session(&self, req: &CreateSessionRequest) -> Result<CreateSessionResponse> {
        let url = format!("{}/api/sessions", self.base_url);
        self.post_json(&url, req)
    }

    pub fn promote_session(&self, req: &PromoteSessionRequest) -> Result<PromoteSessionResponse> {
        let url = format!("{}/api/sessions/promote", self.base_url);
        self.post_json(&url, req)
    }

    pub fn demote_session(&self, req: &DemoteSessionRequest) -> Result<DemoteSessionResponse> {
        let url = format!("{}/api/sessions/demote", self.base_url);
        self.post_json(&url, req)
    }

    pub fn unlink_session(&self, req: &UnlinkSessionRequest) -> Result<UnlinkSessionResponse> {
        let url = format!("{}/api/sessions/unlink", self.base_url);
        self.post_json(&url, req)
    }

    pub fn delete_session(&self, req: &DeleteSessionRequest) -> Result<DeleteSessionResponse> {
        let url = format!("{}/api/sessions/delete", self.base_url);
        self.post_json(&url, req)
    }

    pub fn delete_session_by_id(&self, session_id: &str) -> Result<DeleteSessionResponse> {
        let url = format!("{}/api/sessions/{}", self.base_url, session_id);
        self.delete_json(&url)
    }

    pub fn save_session_memo(&self, req: &SaveSessionMemoRequest) -> Result<SaveSessionMemoResponse> {
        let url = format!("{}/api/sessions/memo", self.base_url);
        self.post_json(&url, req)
    }

    pub fn set_foreground_session(
        &self,
        req: &SetForegroundSessionRequest,
    ) -> Result<SetForegroundSessionResponse> {
        let url = format!("{}/api/sessions/foreground", self.base_url);
        self.post_json(&url, req)
    }

    pub fn set_session_phase(
        &self,
        req: &SetSessionPhaseRequest,
    ) -> Result<SetSessionPhaseResponse> {
        let url = format!("{}/api/sessions/phase", self.base_url);
        self.post_json(&url, req)
    }

    pub fn resume_command(
        &self,
        session_id: &str,
        project_root: Option<&str>,
    ) -> Result<GetResumeCommandResponse> {
        let mut url = Url::parse(&format!(
            "{}/api/sessions/{}/resume-command",
            self.base_url, session_id
        ))?;
        if let Some(root) = project_root {
            url.query_pairs_mut().append_pair("projectRoot", root);
        }
        self.get_json(url.as_str())
    }

    // ========================================================================
    // Transcripts
    // ========================================================================

    pub fn transcript(&self, req: &GetTranscriptRequest) -> Result<GetTranscriptResponse> {
        let mut url = Url::parse(&format!(
            "{}/api/transcript/{}/{}",
            self.base_url, req.harness, req.harness_session_id
        ))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(ref path) = req.transcript_path {
                q.append_pair("transcriptPath", path);
            }
            if let Some(max) = req.max_messages {
                q.append_pair("maxMessages", &max.to_string());
            }
            if let Some(before) = req.before {
                q.append_pair("before", &before.to_string());
            }
        }
        self.get_json(url.as_str())
    }

    // ========================================================================
    // Turn Stream & Cancel
    // ========================================================================

    /// Stream a turn via Server-Sent Events (SSE).
    ///
    /// Spawns a background reader thread that deserializes `OrchestratorEvent`s
    /// and delivers them through an MPSC receiver. Returns a `TurnStreamHandle`
    /// that can be used to monitor or cancel the turn.
    pub fn stream_turn(
        &self,
        req: &TurnStreamRequest,
    ) -> Result<(TurnStreamHandle, Receiver<StreamMessage>)> {
        let url = format!("{}/api/turn/stream", self.base_url);
        let http_req = self
            .apply_headers(self.agent.post(&url))
            .set("Accept", "text/event-stream")
            .set("Content-Type", "application/json");

        let resp = match http_req.send_json(req) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(NexusError::HttpStatus { status: code, body });
            }
            Err(ureq::Error::Transport(t)) => return Err(NexusError::Http(t.to_string())),
        };

        let (tx, rx) = mpsc::channel();
        let turn_id = Arc::new(Mutex::new(None));
        let abort_flag = Arc::new(AtomicBool::new(false));

        let handle = TurnStreamHandle {
            client: self.clone(),
            turn_id: Arc::clone(&turn_id),
            session_id: req.session_id.clone(),
            abort_flag: Arc::clone(&abort_flag),
        };

        // Background reader thread
        let mut reader = resp.into_reader();
        let thread_turn_id = Arc::clone(&turn_id);
        let thread_abort = Arc::clone(&abort_flag);

        thread::spawn(move || {
            let mut parser = SseParser::new();
            let mut buf = [0u8; 4096];

            loop {
                if thread_abort.load(Ordering::Relaxed) {
                    let _ = tx.send(StreamMessage::Done);
                    break;
                }

                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF
                        for msg in parser.finish() {
                            let _ = tx.send(msg);
                        }
                        break;
                    }
                    Ok(n) => {
                        let msgs = parser.feed(&buf[..n]);
                        for msg in msgs {
                            if let StreamMessage::Event(ref evt) = msg {
                                if let Some(ref tid) = evt.turn_id {
                                    let mut guard = thread_turn_id.lock().unwrap();
                                    if guard.is_none() {
                                        *guard = Some(tid.clone());
                                    }
                                }
                            }
                            if tx.send(msg).is_err() {
                                // Receiver dropped
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(StreamMessage::Error(format!("Read error: {e}")));
                        let _ = tx.send(StreamMessage::Done);
                        break;
                    }
                }
            }
        });

        Ok((handle, rx))
    }

    pub fn cancel_turn(&self, req: &TurnCancelRequest) -> Result<TurnCancelResponse> {
        let url = format!("{}/api/turn/cancel", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Plan, Board, Dispatch, Debate
    // ========================================================================

    pub fn board(&self, req: &GetBoardRequest) -> Result<GetBoardResponse> {
        let mut url = Url::parse(&format!("{}/api/board", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(ref sess) = req.session_id {
                q.append_pair("sessionId", sess);
            }
        }
        self.get_json(url.as_str())
    }

    pub fn get_plan(
        &self,
        project_root: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<GetPlanResponse> {
        let mut url = Url::parse(&format!("{}/api/plan", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(root) = project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(sess) = session_id {
                q.append_pair("sessionId", sess);
            }
        }
        self.get_json(url.as_str())
    }

    pub fn save_plan(&self, req: &SavePlanRequest) -> Result<SavePlanResponse> {
        let url = format!("{}/api/plan", self.base_url);
        self.post_json(&url, req)
    }

    pub fn sync_plan(&self, req: &PlanSyncRequest) -> Result<PlanSyncResponse> {
        let url = format!("{}/api/plan/sync", self.base_url);
        self.post_json(&url, req)
    }

    pub fn dispatch(&self, req: &DispatchRequest) -> Result<DispatchResponse> {
        let url = format!("{}/api/dispatch", self.base_url);
        self.post_json(&url, req)
    }

    pub fn debate(&self, req: &DebateRequest) -> Result<DebateResponse> {
        let url = format!("{}/api/debate", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Worktree diff, approve, reject
    // ========================================================================

    pub fn worktree_diff(&self, req: &WorktreeDiffRequest) -> Result<WorktreeDiffResponse> {
        let mut url = Url::parse(&format!("{}/api/worktree/diff", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(ref task) = req.task_id {
                q.append_pair("taskId", task);
            }
            if let Some(turn) = req.turn {
                q.append_pair("turn", &turn.to_string());
            }
            if let Some(ref wt) = req.wt_path {
                q.append_pair("wtPath", wt);
            }
            if let Some(ref base) = req.base {
                q.append_pair("base", base);
            }
        }
        self.get_json(url.as_str())
    }

    pub fn approve_task(&self, req: &ApproveTaskRequest) -> Result<ApproveTaskResponse> {
        let url = format!("{}/api/tasks/approve", self.base_url);
        self.post_json(&url, req)
    }

    pub fn reject_task(&self, req: &RejectTaskRequest) -> Result<RejectTaskResponse> {
        let url = format!("{}/api/tasks/reject", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Subagents
    // ========================================================================

    pub fn subagents(&self, req: &GetSubagentsRequest) -> Result<GetSubagentsResponse> {
        let mut url = Url::parse(&format!("{}/api/subagents", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(ref sess) = req.session_id {
                q.append_pair("sessionId", sess);
            }
            if let Some(ref task) = req.task_id {
                q.append_pair("taskId", task);
            }
        }
        self.get_json(url.as_str())
    }

    pub fn subagent_logs(&self, req: &SubagentLogTailRequest) -> Result<SubagentLogTailResponse> {
        let mut url = Url::parse(&format!("{}/api/subagents/logs", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            q.append_pair("subSessionId", &req.sub_session_id);
            if let Some(tail) = req.tail_lines {
                q.append_pair("tailLines", &tail.to_string());
            }
        }
        self.get_json(url.as_str())
    }

    pub fn subagent_action(&self, req: &SubagentActionRequest) -> Result<SubagentActionResponse> {
        let url = format!("{}/api/subagent-action", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Terminal Open
    // ========================================================================

    pub fn terminal_open(&self, req: &TerminalOpenRequest) -> Result<TerminalOpenResponse> {
        let url = format!("{}/api/terminal/open", self.base_url);
        self.post_json(&url, req)
    }

    // ========================================================================
    // Harness Commands & Workspace FS List
    // ========================================================================

    pub fn harness_commands(
        &self,
        req: &GetHarnessCommandsRequest,
    ) -> Result<GetHarnessCommandsResponse> {
        let mut url = Url::parse(&format!("{}/api/harness-commands", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref harness) = req.harness {
                q.append_pair("harness", harness);
            }
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
        }
        self.get_json(url.as_str())
    }

    pub fn fs_list(&self, req: &FsListRequest) -> Result<FsListResponse> {
        let mut url = Url::parse(&format!("{}/api/fs/list", self.base_url))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(ref root) = req.project_root {
                q.append_pair("projectRoot", root);
            }
            if let Some(mode) = req.mode {
                let mode_str = match mode {
                    FsListMode::Files => "files",
                    FsListMode::Dirs => "dirs",
                };
                q.append_pair("mode", mode_str);
            }
            if let Some(ref prefix) = req.prefix {
                q.append_pair("prefix", prefix);
            }
            if let Some(limit) = req.limit {
                q.append_pair("limit", &limit.to_string());
            }
        }
        self.get_json(url.as_str())
    }
}

/// Handle for monitoring or canceling an ongoing turn stream.
#[derive(Clone, Debug)]
pub struct TurnStreamHandle {
    client: NexusClient,
    turn_id: Arc<Mutex<Option<String>>>,
    session_id: Option<String>,
    abort_flag: Arc<AtomicBool>,
}

impl TurnStreamHandle {
    /// Resolved turn ID, if assigned by the sidecar server.
    pub fn turn_id(&self) -> Option<String> {
        self.turn_id.lock().unwrap().clone()
    }

    /// Whether this stream was signaled to cancel.
    pub fn is_aborted(&self) -> bool {
        self.abort_flag.load(Ordering::Relaxed)
    }

    /// Cancel this turn via `POST /api/turn/cancel`.
    pub fn cancel(&self) -> Result<TurnCancelResponse> {
        self.abort_flag.store(true, Ordering::Relaxed);
        let turn_id = match self.turn_id() {
            Some(id) => id,
            None => {
                return Ok(TurnCancelResponse {
                    success: true,
                    message: Some("Stream aborted before server assigned a turnId".into()),
                    error: None,
                });
            }
        };

        self.client.cancel_turn(&TurnCancelRequest {
            turn_id,
            session_id: self.session_id.clone(),
        })
    }
}
