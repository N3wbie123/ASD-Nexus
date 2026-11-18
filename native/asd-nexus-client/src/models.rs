//! ASD-Nexus Protocol Models and DTOs
//!
//! Mirrors `src/shared/protocol.ts` 1:1 with exact camelCase Serde wire encoding.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// OrchestratorEvent — normalized 5-harness streaming event contract
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratorEventKind {
    Thinking,
    ToolUse,
    ToolResult,
    Text,
    Status,
    Usage,
    Error,
    PermissionRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratorTurnStatus {
    Running,
    Done,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolUsePayload {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsagePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionItem {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestPayload {
    pub request_id: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<String>>,
    #[serde(default)]
    pub options: Vec<PermissionOptionItem>,
}

/// Single normalized SSE event emitted on /api/turn/stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorEvent {
    pub kind: OrchestratorEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<ToolUsePayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResultPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<OrchestratorTurnStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsagePayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_request: Option<PermissionRequestPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

// ============================================================================
// Sidecar lifecycle — `--port 0` ready handshake
// ============================================================================

pub const SIDECAR_READY_PROTOCOL: &str = "asd-nexus-sidecar";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarReadyInfo {
    pub protocol: String,
    pub protocol_version: u32,
    pub pid: u32,
    pub host: String,
    pub port: u16,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_required: Option<bool>,
}

// ============================================================================
// Health
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHealthResponse {
    pub ok: bool,
    pub version: String,
    pub default_project_root: String,
    pub pid: u32,
}

// ============================================================================
// Projects
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryItem {
    pub project_root: String,
    pub project_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_disk_check: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_refresh: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectSummaryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectProjectRequest {
    pub project_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialize: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectProjectResponse {
    pub success: bool,
    pub project_root: String,
    pub project_hash: String,
    #[serde(default)]
    pub paths: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProjectPathsRequest {
    pub project_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProjectPathsResponse {
    pub project_root: String,
    pub paths: HashMap<String, String>,
}

// ============================================================================
// Models
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetModelsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_refresh: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetModelsResponse {
    pub models: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<String>,
}

// ============================================================================
// Routing
// ============================================================================

pub type RoutingConfigPayload = serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRoutingRequest {
    pub project_root: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRoutingResponse {
    pub project_root: String,
    pub routing: RoutingConfigPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRoutingRequest {
    pub project_root: String,
    pub action: String, // "save" | "reset"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_config: Option<RoutingConfigPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRoutingResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Sessions
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSessionItem {
    pub harness: String,
    pub harness_session_id: String,
    pub project_root: String,
    pub updated_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asd_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asd_phase: Option<String>,
    #[serde(default)]
    pub is_promoted_to_asd: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetSessionsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_local_transcripts: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSessionsResponse {
    pub sessions: Vec<DiscoveredSessionItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteSessionRequest {
    pub project_root: String,
    pub harness: String,
    pub harness_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make_foreground: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteSessionResponse {
    pub success: bool,
    pub session_id: String,
    pub created: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkSessionRequest {
    pub project_root: String,
    pub asd_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_asd_session: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkSessionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoteSessionRequest {
    pub project_root: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoteSessionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionRequest {
    pub project_root: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSessionMemoRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    pub memo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSessionMemoResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetForegroundSessionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetForegroundSessionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Planning,
    Running,
    Paused,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionPhaseRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    pub phase: SessionPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionPhaseResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make_foreground: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionResponse {
    pub success: bool,
    pub session_id: String,
    pub phase: SessionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionAutoApproveRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    pub auto_approve: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionAutoApproveResponse {
    pub success: bool,
    pub session_id: String,
    pub auto_approve: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResumeCommandResponse {
    pub success: bool,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_session_id: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub cli_string: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Transcripts — 4-category normalized history
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptCategory {
    User,
    Assistant,
    Thinking,
    ToolUse,
    ToolResult,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedTranscriptMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<TranscriptCategory>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<ToolUsePayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResultPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTranscriptRequest {
    pub harness: String,
    pub harness_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTranscriptResponse {
    pub harness: String,
    pub harness_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub messages: Vec<NormalizedTranscriptMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Turn stream / cancel
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStreamRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStreamAccepted {
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub stream_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelRequest {
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPermissionDecisionRequest {
    pub turn_id: String,
    pub request_id: String,
    pub outcome: String, // "selected" | "cancelled"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPermissionDecisionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Plan / board / dispatch / debate
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTaskItem {
    pub id: String,
    pub subject: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    pub tasks: Vec<BoardTaskItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanSyncRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_md_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSyncResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks_updated: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPlanResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub plan_md_path: String,
    pub exists: bool,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlanRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_to_db: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlanResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks_updated: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>, // "parallel_spawn" | "claim_only"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    pub topic: String,
    pub proposal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebateResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Worktree diff / approve / reject
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiffFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiffRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wt_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>, // "HEAD" | "merge-base"
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiffResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>, // "working" | "committed"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub files: Vec<WorktreeDiffFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveTaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveTaskResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectTaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectTaskResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Subagents
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRecordItem {
    pub sub_session_id: String,
    pub parent_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetSubagentsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSubagentsResponse {
    pub subagents: Vec<SubagentRecordItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentLogTailRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub sub_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentLogTailResponse {
    pub sub_session_id: String,
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentControlActionKind {
    Kill,
    Retry,
    Fallback,
    Custom,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentActionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub session_id: String,
    pub sub_session_id: String,
    pub action: SubagentControlActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentActionResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Terminal open
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalKind {
    WindowsTerminal,
    Powershell,
    Cmd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenRequest {
    pub project_root: String,
    pub session_id: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launched: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// Harness Slash Commands & Skills Discovery
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCommandSource {
    Builtin,
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCommandKind {
    Command,
    Skill,
    Workflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCommandItem {
    pub name: String,
    pub description: String,
    pub harness: String,
    pub source: HarnessCommandSource,
    pub kind: HarnessCommandKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetHarnessCommandsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHarnessCommandsResponse {
    pub commands: Vec<HarnessCommandItem>,
}

// ============================================================================
// Workspace Files & Directories Listing (GET /api/fs/list)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsListMode {
    Files,
    Dirs,
}

impl Default for FsListMode {
    fn default() -> Self {
        Self::Files
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsListEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FsListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<FsListMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsListResponse {
    pub project_root: String,
    pub mode: FsListMode,
    pub base_dir: String,
    pub entries: Vec<FsListEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_event_roundtrip() {
        let event = OrchestratorEvent {
            kind: OrchestratorEventKind::ToolUse,
            session_id: Some("s_123".into()),
            turn_id: Some("t_456".into()),
            harness: Some("claude-code".into()),
            text: None,
            thinking: None,
            tool_use: Some(ToolUsePayload {
                name: "read_file".into(),
                id: Some("call_1".into()),
                input: Some("{\"path\":\"test.txt\"}".into()),
            }),
            tool_result: None,
            status: Some(OrchestratorTurnStatus::Running),
            usage: Some(TokenUsagePayload {
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
                cost_usd: Some(0.002),
            }),
            permission_request: None,
            error: None,
            timestamp: Some("2026-11-13T09:00:00Z".into()),
            raw: None,
        };

        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"toolUse\":"));
        assert!(json.contains("\"sessionId\":\"s_123\""));
        assert!(json.contains("\"kind\":\"tool_use\""));
        assert!(json.contains("\"status\":\"running\""));

        let deserialized: OrchestratorEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_sidecar_ready_info_roundtrip() {
        let raw_json = r#"{
            "protocol": "asd-nexus-sidecar",
            "protocolVersion": 1,
            "pid": 1234,
            "host": "127.0.0.1",
            "port": 54321,
            "baseUrl": "http://127.0.0.1:54321",
            "projectRoot": "C:\\repo",
            "version": "2.6.0",
            "tokenRequired": false,
            "startedAt": "2026-11-13T09:00:00.000Z"
        }"#;

        let info: SidecarReadyInfo = serde_json::from_str(raw_json).expect("deserialize ready");
        assert_eq!(info.protocol, SIDECAR_READY_PROTOCOL);
        assert_eq!(info.port, 54321);
        assert_eq!(info.pid, 1234);
        assert_eq!(info.base_url.as_deref(), Some("http://127.0.0.1:54321"));
        assert_eq!(info.token_required, Some(false));
    }

    #[test]
    fn test_all_dtos_serialization() {
        let health = GetHealthResponse {
            ok: true,
            version: "2.6.0".into(),
            default_project_root: "C:\\test".into(),
            pid: 999,
        };
        let health_json = serde_json::to_string(&health).unwrap();
        assert!(health_json.contains("\"defaultProjectRoot\""));

        let board = GetBoardResponse {
            session_id: Some("s1".into()),
            phase: Some("planning".into()),
            progress: Some("1/2".into()),
            tasks: vec![BoardTaskItem {
                id: "T1".into(),
                subject: "Test".into(),
                status: "done".into(),
                assignee_role: Some("architect".into()),
                difficulty: None,
                turn: Some(1),
                review_verdict: None,
                summary: Some("done".into()),
            }],
        };
        let board_json = serde_json::to_string(&board).unwrap();
        assert!(board_json.contains("\"assigneeRole\""));

        let term = TerminalOpenResponse {
            success: true,
            command: Some("powershell".into()),
            args: Some(vec!["-NoExit".into()]),
            cli_string: Some("powershell -NoExit".into()),
            cwd: Some("C:\\".into()),
            launched: Some(false),
            error: None,
        };
        let term_json = serde_json::to_string(&term).unwrap();
        assert!(term_json.contains("\"cliString\""));

        let cmds = GetHarnessCommandsResponse {
            commands: vec![HarnessCommandItem {
                name: "compact".into(),
                description: "Compact context".into(),
                harness: "claude-code".into(),
                source: HarnessCommandSource::Builtin,
                kind: HarnessCommandKind::Command,
                path: None,
            }],
        };
        let cmds_json = serde_json::to_string(&cmds).unwrap();
        assert!(cmds_json.contains("\"source\":\"builtin\""));
        assert!(cmds_json.contains("\"kind\":\"command\""));
        let cmds_de: GetHarnessCommandsResponse = serde_json::from_str(&cmds_json).unwrap();
        assert_eq!(cmds, cmds_de);

        let fs_resp = FsListResponse {
            project_root: "C:/repo".into(),
            mode: FsListMode::Files,
            base_dir: "C:/repo".into(),
            entries: vec![FsListEntry {
                path: "src/main.rs".into(),
                name: "main.rs".into(),
                is_dir: false,
            }],
            truncated: Some(false),
        };
        let fs_json = serde_json::to_string(&fs_resp).unwrap();
        assert!(fs_json.contains("\"projectRoot\":\"C:/repo\""));
        assert!(fs_json.contains("\"isDir\":false"));
        let fs_de: FsListResponse = serde_json::from_str(&fs_json).unwrap();
        assert_eq!(fs_resp, fs_de);
    }

    #[test]
    fn test_permission_request_event_roundtrip() {
        let event = OrchestratorEvent {
            kind: OrchestratorEventKind::PermissionRequest,
            session_id: Some("s_perm".into()),
            turn_id: Some("t_perm".into()),
            harness: Some("claude-code".into()),
            text: None,
            thinking: None,
            tool_use: None,
            tool_result: None,
            status: None,
            usage: None,
            permission_request: Some(PermissionRequestPayload {
                request_id: "req_1".into(),
                tool_name: "Bash".into(),
                tool_kind: Some("execute".into()),
                tool_input: Some("{\"command\":\"ls\"}".into()),
                locations: Some(vec!["src/main.rs".into()]),
                options: vec![
                    PermissionOptionItem {
                        option_id: "allow_once".into(),
                        name: "Allow once".into(),
                        kind: "allow_once".into(),
                    },
                    PermissionOptionItem {
                        option_id: "reject_once".into(),
                        name: "Reject".into(),
                        kind: "reject_once".into(),
                    },
                ],
            }),
            error: None,
            timestamp: Some("2026-11-18T07:50:00Z".into()),
            raw: None,
        };

        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"kind\":\"permission_request\""));
        assert!(json.contains("\"permissionRequest\":"));
        assert!(json.contains("\"requestId\":\"req_1\""));
        assert!(json.contains("\"toolName\":\"Bash\""));
        assert!(json.contains("\"optionId\":\"allow_once\""));

        let deserialized: OrchestratorEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_auto_approve_and_permission_decision_roundtrip() {
        let create_req = CreateSessionRequest {
            project_root: Some("C:/repo".into()),
            harness: Some("claude-code".into()),
            auto_approve: Some(false),
            ..Default::default()
        };
        let create_req_json = serde_json::to_string(&create_req).unwrap();
        assert!(create_req_json.contains("\"autoApprove\":false"));
        let create_req_de: CreateSessionRequest = serde_json::from_str(&create_req_json).unwrap();
        assert_eq!(create_req, create_req_de);

        let create_resp = CreateSessionResponse {
            success: true,
            session_id: "s_1".into(),
            phase: SessionPhase::Planning,
            auto_approve: Some(false),
            error: None,
        };
        let create_resp_json = serde_json::to_string(&create_resp).unwrap();
        assert!(create_resp_json.contains("\"autoApprove\":false"));
        let create_resp_de: CreateSessionResponse =
            serde_json::from_str(&create_resp_json).unwrap();
        assert_eq!(create_resp, create_resp_de);

        let auto_req = SetSessionAutoApproveRequest {
            project_root: Some("C:/repo".into()),
            session_id: "s_1".into(),
            auto_approve: true,
        };
        let auto_req_json = serde_json::to_string(&auto_req).unwrap();
        assert!(auto_req_json.contains("\"sessionId\":\"s_1\""));
        assert!(auto_req_json.contains("\"autoApprove\":true"));
        let auto_req_de: SetSessionAutoApproveRequest =
            serde_json::from_str(&auto_req_json).unwrap();
        assert_eq!(auto_req, auto_req_de);

        let auto_resp = SetSessionAutoApproveResponse {
            success: true,
            session_id: "s_1".into(),
            auto_approve: true,
            error: None,
        };
        let auto_resp_json = serde_json::to_string(&auto_resp).unwrap();
        let auto_resp_de: SetSessionAutoApproveResponse =
            serde_json::from_str(&auto_resp_json).unwrap();
        assert_eq!(auto_resp, auto_resp_de);

        let perm_req = TurnPermissionDecisionRequest {
            turn_id: "t_1".into(),
            request_id: "req_1".into(),
            outcome: "selected".into(),
            option_id: Some("allow_once".into()),
        };
        let perm_req_json = serde_json::to_string(&perm_req).unwrap();
        assert!(perm_req_json.contains("\"turnId\":\"t_1\""));
        assert!(perm_req_json.contains("\"requestId\":\"req_1\""));
        assert!(perm_req_json.contains("\"optionId\":\"allow_once\""));
        let perm_req_de: TurnPermissionDecisionRequest =
            serde_json::from_str(&perm_req_json).unwrap();
        assert_eq!(perm_req, perm_req_de);

        let perm_resp = TurnPermissionDecisionResponse {
            success: true,
            error: None,
        };
        let perm_resp_json = serde_json::to_string(&perm_resp).unwrap();
        let perm_resp_de: TurnPermissionDecisionResponse =
            serde_json::from_str(&perm_resp_json).unwrap();
        assert_eq!(perm_resp, perm_resp_de);
    }
}
