//! Executes queued [`Action`]s against the sidecar via [`NexusClient`].

use asd_nexus_client::*;

use crate::app::{phase_name, terminal_name, Action, ActiveStream, App, HARNESSES, TERMINALS};
use crate::timeline::{Entry, EntryKind};

fn outcome(app: &mut App, label: &str, success: bool, error: Option<String>) -> bool {
    if success {
        app.set_status(format!("{label}: ok"));
    } else {
        app.set_status(format!("{label}: {}", error.unwrap_or_else(|| "failed".into())));
    }
    success
}

fn try_<T>(app: &mut App, label: &str, r: Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            app.set_status(format!("{label} failed: {e}"));
            None
        }
    }
}

pub fn run_pending(app: &mut App, client: &NexusClient) {
    // Actions may queue follow-ups; bound the loop to avoid runaway chains.
    for _ in 0..8 {
        let actions = app.take_actions();
        if actions.is_empty() {
            break;
        }
        for a in actions {
            execute(app, client, a);
        }
    }
}

fn root(app: &App) -> Option<String> {
    Some(app.project_root.clone())
}

pub fn execute(app: &mut App, client: &NexusClient, action: Action) {
    match action {
        Action::LoadInitial => {
            if let Some(h) = try_(app, "health", client.health()) {
                app.server_label = format!("{} v{} pid {}", client.base_url(), h.version, h.pid);
            }
            execute(app, client, Action::RefreshProjects);
            execute(app, client, Action::RefreshSessions);
            execute(app, client, Action::RefreshModels { force: false });
            execute(app, client, Action::RefreshHarnessCommands);
            app.set_status("Connected · / commands · @ files · ! shell · Ctrl+P palette");
        }
        Action::RefreshProjects => {
            let req = ListProjectsRequest { skip_disk_check: Some(true), force_refresh: None };
            if let Some(r) = try_(app, "projects", client.projects(&req)) {
                app.projects = r.projects;
                app.project_sel = app
                    .projects
                    .iter()
                    .position(|p| p.project_root == app.project_root)
                    .unwrap_or(0);
            }
        }
        Action::SelectProject(project_root) => {
            let req = SelectProjectRequest { project_root, initialize: None };
            if let Some(r) = try_(app, "select project", client.select_project(&req)) {
                if outcome(app, "select project", r.success, r.error) {
                    app.project_root = r.project_root;
                    app.current = None;
                    app.current_asd = None;
                    app.cli_session = None;
                    app.new_session_draft = true;
                    app.pending_permissions.clear();
                    app.permission_sel = 0;
                    app.timeline.clear();
                    app.timeline_sel = 0;
                    app.timeline_scroll = None;
                    app.board = None;
                    app.session_sel = 0;
                    app.file_candidates.clear();
                    app.dir_candidates.clear();
                    execute(app, client, Action::RefreshProjects);
                    execute(app, client, Action::RefreshSessions);
                    execute(app, client, Action::RefreshHarnessCommands);
                }
            }
        }
        Action::RefreshSessions => {
            let req = GetSessionsRequest { project_root: root(app), limit: Some(500), sync_local_transcripts: None };
            if let Some(r) = try_(app, "sessions", client.sessions(&req)) {
                let n = r.sessions.len();
                app.set_sessions(r);
                app.set_status(format!("{n} sessions"));
            }
        }
        Action::OpenSession(s) => {
            let harness_changed = app.harness != s.harness;
            let req = GetTranscriptRequest {
                harness: s.harness.clone(),
                harness_session_id: s.harness_session_id.clone(),
                project_root: root(app),
                transcript_path: s.transcript_path.clone(),
                max_messages: Some(300),
                before: None,
            };
            app.harness = s.harness.clone();
            app.picker_harness = HARNESSES.iter().position(|h| *h == app.harness).unwrap_or(0);
            if s.model.is_some() {
                app.model = s.model.clone();
            }
            app.current_asd = s.asd_session_id.clone();
            app.auto_approve = s.auto_approve.unwrap_or(true);
            app.new_session_draft = false;
            app.pending_permissions.clear();
            app.permission_sel = 0;
            app.current = Some(*s);
            app.timeline_sel = 0;
            app.timeline_scroll = None;
            if let Some(r) = try_(app, "transcript", client.transcript(&req)) {
                app.timeline.load_transcript(&r.messages);
                if let Some(e) = r.error {
                    app.timeline.push(Entry::new(EntryKind::Error, e));
                }
                app.timeline_sel = app.timeline.entries.len().saturating_sub(1);
                app.set_status(format!("Loaded {} messages", r.messages.len()));
            }
            if harness_changed {
                execute(app, client, Action::RefreshHarnessCommands);
            }
        }
        Action::CreateSession { title } => {
            let req = CreateSessionRequest {
                project_root: root(app),
                harness: Some(app.harness.clone()),
                model: app.model.clone(),
                title,
                memo: None,
                make_foreground: Some(true),
                auto_approve: Some(app.auto_approve),
            };
            if let Some(r) = try_(app, "create session", client.create_session(&req)) {
                if outcome(app, "create session", r.success, r.error) {
                    app.current_asd = Some(r.session_id.clone());
                    app.current = None;
                    app.new_session_draft = false;
                    app.auto_approve = r.auto_approve.unwrap_or(app.auto_approve);
                    app.pending_permissions.clear();
                    app.permission_sel = 0;
                    app.timeline.clear();
                    app.timeline_sel = 0;
                    app.timeline_scroll = None;
                    execute(app, client, Action::RefreshSessions);
                    app.set_status(format!("Created session {}", r.session_id));
                }
            }
        }
        Action::Promote(s) => {
            let req = PromoteSessionRequest {
                project_root: app.project_root.clone(),
                harness: s.harness.clone(),
                harness_session_id: s.harness_session_id.clone(),
                model: s.model.clone(),
                title: s.summary.clone(),
                memo: None,
                transcript_path: s.transcript_path.clone(),
                make_foreground: Some(false),
                source: Some("asd-nexus-cli".into()),
            };
            if let Some(r) = try_(app, "promote", client.promote_session(&req)) {
                if outcome(app, "promote", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                    app.set_status(format!("Promoted → {}", r.session_id));
                }
            }
        }
        Action::Demote(session_id) => {
            let req = DemoteSessionRequest { project_root: app.project_root.clone(), session_id };
            if let Some(r) = try_(app, "demote", client.demote_session(&req)) {
                if outcome(app, "demote", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                    if let Some(cli) = r.cli_string {
                        app.set_status(format!("Demoted. Resume with: {cli}"));
                    }
                }
            }
        }
        Action::Unlink(asd_session_id) => {
            let req = UnlinkSessionRequest {
                project_root: app.project_root.clone(),
                asd_session_id,
                delete_asd_session: Some(false),
            };
            if let Some(r) = try_(app, "unlink", client.unlink_session(&req)) {
                if outcome(app, "unlink", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                }
            }
        }
        Action::Delete(session_id) => {
            let req = DeleteSessionRequest { project_root: app.project_root.clone(), session_id: session_id.clone() };
            if let Some(r) = try_(app, "delete", client.delete_session(&req)) {
                if outcome(app, "delete", r.success, r.error) {
                    if app.current_asd.as_deref() == Some(&session_id) {
                        app.current_asd = None;
                    }
                    execute(app, client, Action::RefreshSessions);
                }
            }
        }
        Action::SaveMemo { session_id, memo } => {
            let req = SaveSessionMemoRequest { project_root: root(app), session_id, memo };
            if let Some(r) = try_(app, "memo", client.save_session_memo(&req)) {
                if outcome(app, "memo", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                }
            }
        }
        Action::SetForeground(session_id) => {
            let req = SetForegroundSessionRequest { project_root: root(app), session_id };
            if let Some(r) = try_(app, "foreground", client.set_foreground_session(&req)) {
                if outcome(app, "foreground", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                }
            }
        }
        Action::SetPhase { session_id, phase } => {
            let req = SetSessionPhaseRequest { project_root: root(app), session_id, phase };
            if let Some(r) = try_(app, "phase", client.set_session_phase(&req)) {
                if outcome(app, "phase", r.success, r.error) {
                    execute(app, client, Action::RefreshSessions);
                    app.set_status(format!("Phase → {}", phase_name(phase)));
                }
            }
        }
        Action::SetAutoApprove(enabled) => {
            if let Some(sid) = app.active_session_id() {
                let req = SetSessionAutoApproveRequest {
                    project_root: root(app),
                    session_id: sid,
                    auto_approve: enabled,
                };
                if let Some(r) = try_(app, "auto-approve", client.set_session_auto_approve(&req)) {
                    if outcome(app, "auto-approve", r.success, r.error) {
                        app.auto_approve = r.auto_approve;
                        app.set_status(format!(
                            "Auto-approve: {}",
                            if r.auto_approve { "ON" } else { "OFF" }
                        ));
                    }
                }
            }
        }
        Action::RespondPermission { turn_id, request_id, outcome: decision_outcome, option_id } => {
            let req = TurnPermissionDecisionRequest {
                turn_id,
                request_id,
                outcome: decision_outcome,
                option_id,
            };
            if let Some(r) = try_(app, "permission", client.submit_turn_permission(&req)) {
                outcome(app, "permission", r.success, r.error);
            }
        }
        Action::RefreshModels { force } => {
            let r = if force {
                client.refresh_models()
            } else {
                client.models(&GetModelsRequest { force_refresh: None })
            };
            if let Some(r) = try_(app, "models", r) {
                app.models = r.models;
                if force {
                    app.set_status("Models refreshed");
                }
            }
        }
        Action::RefreshHarnessCommands => {
            let req = GetHarnessCommandsRequest {
                harness: Some(app.harness.clone()),
                project_root: root(app),
            };
            if let Some(r) = try_(app, "harness commands", client.harness_commands(&req)) {
                app.harness_commands = r.commands;
            }
        }
        Action::ListFiles { prefix } => {
            let req = FsListRequest {
                project_root: root(app),
                mode: Some(FsListMode::Files),
                prefix,
                limit: Some(100),
            };
            if let Some(r) = try_(app, "fs files", client.fs_list(&req)) {
                app.file_candidates = r.entries;
                let max = app.filtered_files().len().saturating_sub(1);
                app.file_sel = app.file_sel.min(max);
            }
        }
        Action::ListDirs { prefix } => {
            let req = FsListRequest {
                project_root: root(app),
                mode: Some(FsListMode::Dirs),
                prefix,
                limit: Some(100),
            };
            if let Some(r) = try_(app, "fs dirs", client.fs_list(&req)) {
                app.dir_candidates = r.entries;
                let max = app.dir_picker_items().len().saturating_sub(1);
                app.dir_sel = app.dir_sel.min(max);
            }
        }
        Action::RunShell(cmd) => {
            app.execute_shell(&cmd);
        }
        Action::SendPrompt(prompt) => {
            app.new_session_draft = false;
            let mut session_id = app.current_asd.clone().or_else(|| app.cli_session.clone());
            if session_id.is_none() && !app.auto_approve {
                let create_req = CreateSessionRequest {
                    project_root: root(app),
                    harness: Some(app.harness.clone()),
                    model: app.model.clone(),
                    make_foreground: Some(true),
                    auto_approve: Some(false),
                    ..Default::default()
                };
                if let Some(r) = try_(app, "create session", client.create_session(&create_req)) {
                    if r.success {
                        app.current_asd = Some(r.session_id.clone());
                        app.auto_approve = r.auto_approve.unwrap_or(false);
                        session_id = Some(r.session_id);
                    }
                }
            }
            let req = TurnStreamRequest {
                project_root: root(app),
                session_id,
                harness: app.harness.clone(),
                model: app.model.clone(),
                prompt: prompt.clone(),
                source: Some("asd-nexus-cli".into()),
                streaming: Some(true),
            };
            app.timeline.begin_turn(&prompt);
            app.timeline_sel = app.timeline.entries.len().saturating_sub(1);
            app.timeline_scroll = None;
            match client.stream_turn(&req) {
                Ok((handle, rx)) => {
                    app.stream = Some(ActiveStream { handle: Some(handle), rx });
                    app.set_status(format!("Streaming via {}…", app.harness));
                }
                Err(e) => {
                    app.timeline.push(Entry::new(EntryKind::Error, format!("stream failed: {e}")));
                    app.timeline.end_turn(None);
                    app.set_status("Stream failed");
                }
            }
        }
        Action::CancelTurn => {
            let handle = app.stream.as_mut().and_then(|s| s.handle.take());
            match handle {
                Some(h) => match h.cancel() {
                    Ok(r) => {
                        let msg = r.message.or(r.error).unwrap_or_default();
                        app.set_status(format!("Cancel requested {msg}"));
                    }
                    Err(e) => app.set_status(format!("cancel failed: {e}")),
                },
                None => app.set_status("Cancel already requested"),
            }
        }
        Action::RefreshBoard => {
            let req = GetBoardRequest { project_root: root(app), session_id: app.active_session_id() };
            if let Some(b) = try_(app, "board", client.board(&req)) {
                let n = b.tasks.len().saturating_sub(1);
                app.board_sel = app.board_sel.min(n);
                app.diff_sel = app.diff_sel.min(n);
                app.board = Some(b);
            }
        }
        Action::LoadPlan => {
            let sid = app.active_session_id();
            if let Some(p) = try_(app, "plan", client.get_plan(Some(&app.project_root.clone()), sid.as_deref())) {
                app.plan = Some(p);
            }
        }
        Action::SyncPlan => {
            let req = PlanSyncRequest { project_root: root(app), session_id: app.active_session_id(), plan_md_path: None };
            if let Some(r) = try_(app, "plan sync", client.sync_plan(&req)) {
                if outcome(app, "plan sync", r.success, r.error) {
                    app.set_status(format!("Plan synced: {} tasks updated", r.tasks_updated.unwrap_or(0)));
                    execute(app, client, Action::RefreshBoard);
                }
            }
        }
        Action::Dispatch { mode } => {
            let Some(session_id) = app.active_session_id() else {
                app.set_status("dispatch: no ASD session");
                return;
            };
            let req = DispatchRequest { project_root: root(app), session_id, turn: None, mode: Some(mode), task_ids: None };
            if let Some(r) = try_(app, "dispatch", client.dispatch(&req)) {
                if outcome(app, "dispatch", r.success, r.error) {
                    let d = r.dispatched.unwrap_or_default().join(", ");
                    app.set_status(format!("Dispatched: {d} {}", r.message.unwrap_or_default()));
                    execute(app, client, Action::RefreshBoard);
                }
            }
        }
        Action::Debate { topic, proposal } => {
            let Some(session_id) = app.active_session_id() else {
                app.set_status("debate: no ASD session");
                return;
            };
            let req = DebateRequest { project_root: root(app), session_id, topic, proposal };
            if let Some(r) = try_(app, "debate", client.debate(&req)) {
                if outcome(app, "debate", r.success, r.error) {
                    let v = r.verdict.or(r.message).unwrap_or_default();
                    app.set_status(format!("Debate: {v}"));
                }
            }
        }
        Action::RefreshSubagents => {
            let req = GetSubagentsRequest { project_root: root(app), session_id: app.active_session_id(), task_id: None };
            if let Some(r) = try_(app, "subagents", client.subagents(&req)) {
                let first = app.subagents.is_empty() && !r.subagents.is_empty();
                app.subagents = r.subagents;
                app.sub_sel = app.sub_sel.min(app.subagents.len().saturating_sub(1));
                if first {
                    let id = app.subagents[app.sub_sel].sub_session_id.clone();
                    execute(app, client, Action::TailLogs(id));
                }
            }
        }
        Action::TailLogs(sub_session_id) => {
            let req = SubagentLogTailRequest { project_root: root(app), sub_session_id, tail_lines: Some(200) };
            if let Some(r) = try_(app, "logs", client.subagent_logs(&req)) {
                app.logs = r.lines;
                if let Some(e) = r.error {
                    app.logs.push(format!("[error] {e}"));
                }
            }
        }
        Action::SubagentAction { sub_session_id, action, reason } => {
            let Some(session_id) = app.active_session_id() else {
                app.set_status("subagent action: no ASD session");
                return;
            };
            let fallback = action == SubagentControlActionKind::Fallback;
            let req = SubagentActionRequest {
                project_root: root(app),
                session_id,
                sub_session_id,
                action,
                harness: fallback.then(|| app.harness.clone()),
                model: if fallback { app.model.clone() } else { None },
                reason,
            };
            if let Some(r) = try_(app, "subagent action", client.subagent_action(&req)) {
                let label = format!("subagent {action:?}");
                if outcome(app, &label, r.success, r.error) {
                    if let Some(m) = r.message {
                        app.set_status(format!("{label}: {m}"));
                    }
                    execute(app, client, Action::RefreshSubagents);
                }
            }
        }
        Action::LoadDiff(task_id) => {
            let req = WorktreeDiffRequest { project_root: root(app), task_id: Some(task_id), ..Default::default() };
            if let Some(d) = try_(app, "diff", client.worktree_diff(&req)) {
                if let Some(e) = &d.error {
                    app.set_status(format!("diff: {e}"));
                }
                app.diff = Some(d);
            }
        }
        Action::Approve(task_id) => {
            let req = ApproveTaskRequest { project_root: root(app), task_id, summary: None };
            if let Some(r) = try_(app, "approve", client.approve_task(&req)) {
                if outcome(app, "approve", r.success, r.error) {
                    app.set_status(format!("Approved (merged: {})", r.merged.unwrap_or(false)));
                    execute(app, client, Action::RefreshBoard);
                }
            }
        }
        Action::Reject { task_id, reason } => {
            let req = RejectTaskRequest { project_root: root(app), task_id, reason };
            if let Some(r) = try_(app, "reject", client.reject_task(&req)) {
                if outcome(app, "reject", r.success, r.error) {
                    execute(app, client, Action::RefreshBoard);
                }
            }
        }
        Action::LoadRouting => {
            let req = GetRoutingRequest { project_root: app.project_root.clone() };
            if let Some(r) = try_(app, "routing", client.get_routing(&req)) {
                app.routing = Some(r.routing);
            }
        }
        Action::SaveRouting(text) => {
            let value: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    app.set_status(format!("Invalid JSON: {e}"));
                    return;
                }
            };
            let req = SaveRoutingRequest {
                project_root: app.project_root.clone(),
                action: "save".into(),
                routing_config: Some(value),
            };
            if let Some(r) = try_(app, "save routing", client.save_routing(&req)) {
                if outcome(app, "save routing", r.success, r.error) {
                    execute(app, client, Action::LoadRouting);
                }
            }
        }
        Action::ResetRouting => {
            let req = SaveRoutingRequest { project_root: app.project_root.clone(), action: "reset".into(), routing_config: None };
            if let Some(r) = try_(app, "reset routing", client.save_routing(&req)) {
                if outcome(app, "reset routing", r.success, r.error) {
                    execute(app, client, Action::LoadRouting);
                }
            }
        }
        Action::LoadResume(session_id) => {
            let r = client.resume_command(&session_id, Some(&app.project_root.clone()));
            if let Some(r) = try_(app, "resume command", r) {
                app.resume = Some(r);
            }
        }
        Action::LaunchTerminal { session_id } => {
            let kind = TERMINALS[app.terminal_kind];
            let harness = app
                .resume
                .as_ref()
                .map(|r| r.harness.clone())
                .unwrap_or_else(|| app.harness.clone());
            let req = TerminalOpenRequest {
                project_root: app.project_root.clone(),
                session_id,
                harness,
                terminal: Some(kind),
            };
            let Some(spec) = try_(app, "terminal open", client.terminal_open(&req)) else { return };
            if !spec.success {
                outcome(app, "terminal open", false, spec.error);
                return;
            }
            match launch_terminal_spec(&spec, Some(kind)) {
                Ok(child) => app.set_status(format!("Launched {} (pid {})", terminal_name(kind), child.id())),
                Err(e) => app.set_status(format!("launch failed: {e}")),
            }
        }
    }
}
