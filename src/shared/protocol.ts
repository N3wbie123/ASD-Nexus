/**
 * ASD-Nexus Shared Protocol Contracts (src/shared/protocol.ts)
 *
 * Clean-room wire contract between the Node sidecar server (src/server/) and
 * the pure-native Rust clients (GUI + TUI/CLI). Pure TypeScript with zero
 * runtime dependencies and zero Node.js imports, so it can be mirrored or
 * consumed by any client. Only type definitions and string-literal constants
 * live here; all behavior stays in the server.
 */

// ============================================================================
// OrchestratorEvent — normalized 5-harness streaming event contract
// ============================================================================

export const ORCHESTRATOR_EVENT_KINDS = [
  "thinking",
  "tool_use",
  "tool_result",
  "text",
  "status",
  "usage",
  "error",
] as const;

export type OrchestratorEventKind = (typeof ORCHESTRATOR_EVENT_KINDS)[number];

export const ORCHESTRATOR_TURN_STATUSES = [
  "running",
  "done",
  "interrupted",
] as const;

export type OrchestratorTurnStatus = (typeof ORCHESTRATOR_TURN_STATUSES)[number];

export interface ToolUsePayload {
  name: string;
  id?: string;
  input?: string;
}

export interface ToolResultPayload {
  name?: string;
  toolUseId?: string;
  output: string;
  isError?: boolean;
}

export interface TokenUsagePayload {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  costUsd?: number;
}

/** Single normalized SSE event emitted on /api/turn/stream. */
export interface OrchestratorEvent {
  kind: OrchestratorEventKind;
  sessionId?: string;
  turnId?: string;
  harness?: string;
  text?: string;
  thinking?: string;
  toolUse?: ToolUsePayload;
  toolResult?: ToolResultPayload;
  status?: OrchestratorTurnStatus;
  usage?: TokenUsagePayload;
  error?: string;
  timestamp?: string;
  raw?: unknown;
}

// ============================================================================
// Sidecar lifecycle — `--port 0` ready handshake
// ============================================================================

/** Protocol tag printed in the one-line JSON ready message on stdout. */
export const SIDECAR_READY_PROTOCOL = "asd-nexus-sidecar";

/**
 * Emitted by the sidecar server as a single JSON line on stdout once the
 * HTTP listener is bound. The Rust client spawns the sidecar with `--port 0`
 * and waits for this line to learn the resolved port.
 */
export interface SidecarReadyInfo {
  protocol: typeof SIDECAR_READY_PROTOCOL;
  protocolVersion: number;
  pid: number;
  host: string;
  /** Resolved port when started with `--port 0`. */
  port: number;
  startedAt: string;
}

// ============================================================================
// Health
// ============================================================================

export interface GetHealthResponse {
  ok: boolean;
  version: string;
  defaultProjectRoot: string;
  pid: number;
}

// ============================================================================
// Projects
// ============================================================================

export interface ProjectSummaryItem {
  projectRoot: string;
  projectHash: string;
  sessionCount?: number;
  lastActiveAt?: number;
}

export interface ListProjectsRequest {
  skipDiskCheck?: boolean;
  forceRefresh?: boolean;
}

export interface ListProjectsResponse {
  projects: ProjectSummaryItem[];
}

export interface SelectProjectRequest {
  projectRoot: string;
  initialize?: boolean;
}

export interface SelectProjectResponse {
  success: boolean;
  projectRoot: string;
  projectHash: string;
  paths: Record<string, string>;
  error?: string;
}

export interface ResolveProjectPathsRequest {
  projectRoot: string;
}

export interface ResolveProjectPathsResponse {
  projectRoot: string;
  paths: Record<string, string>;
}

// ============================================================================
// Models
// ============================================================================

export interface GetModelsRequest {
  forceRefresh?: boolean;
}

export interface GetModelsResponse {
  models: Record<string, string[]>;
  cachePath?: string;
}

// ============================================================================
// Routing
// ============================================================================

/** Pass-through routing configuration (mirrors core RoutingConfig shape). */
export type RoutingConfigPayload = Record<string, unknown>;

export interface GetRoutingRequest {
  projectRoot: string;
}

export interface GetRoutingResponse {
  projectRoot: string;
  routing: RoutingConfigPayload;
}

export interface SaveRoutingRequest {
  projectRoot: string;
  action: "save" | "reset";
  routingConfig?: RoutingConfigPayload;
}

export interface SaveRoutingResponse {
  success: boolean;
  error?: string;
}

// ============================================================================
// Sessions
// ============================================================================

export interface DiscoveredSessionItem {
  harness: string;
  harnessSessionId: string;
  projectRoot: string;
  updatedAt: number;
  model?: string;
  summary?: string;
  transcriptPath?: string;
  asdSessionId?: string | null;
  asdPhase?: string | null;
  isPromotedToAsd: boolean;
}

export interface GetSessionsRequest {
  projectRoot?: string;
  limit?: number;
  syncLocalTranscripts?: boolean;
}

export interface GetSessionsResponse {
  sessions: DiscoveredSessionItem[];
  activeSessionId?: string | null;
}

export interface PromoteSessionRequest {
  projectRoot: string;
  harness: string;
  harnessSessionId: string;
  model?: string;
  title?: string;
  memo?: string;
  transcriptPath?: string;
  makeForeground?: boolean;
  source?: string;
}

export interface PromoteSessionResponse {
  success: boolean;
  sessionId: string;
  created: boolean;
  error?: string;
}

export interface UnlinkSessionRequest {
  projectRoot: string;
  asdSessionId: string;
  deleteAsdSession?: boolean;
}

export interface UnlinkSessionResponse {
  success: boolean;
  error?: string;
}

export interface DemoteSessionRequest {
  projectRoot: string;
  sessionId: string;
}

export interface DemoteSessionResponse {
  success: boolean;
  cliString?: string;
  error?: string;
}

export interface DeleteSessionRequest {
  projectRoot: string;
  sessionId: string;
}

export interface DeleteSessionResponse {
  success: boolean;
  error?: string;
}

export interface SaveSessionMemoRequest {
  projectRoot?: string;
  sessionId: string;
  memo: string;
}

export interface SaveSessionMemoResponse {
  success: boolean;
  error?: string;
}

export interface SetForegroundSessionRequest {
  projectRoot?: string;
  sessionId: string;
}

export interface SetForegroundSessionResponse {
  success: boolean;
  previousSessionId?: string | null;
  error?: string;
}

export type SessionPhase = "planning" | "running" | "paused" | "done";

export interface SetSessionPhaseRequest {
  projectRoot?: string;
  sessionId: string;
  phase: SessionPhase;
}

export interface CreateSessionRequest {
  projectRoot?: string;
  harness?: string;
  model?: string;
  title?: string;
  memo?: string;
  makeForeground?: boolean;
}

export interface CreateSessionResponse {
  success: boolean;
  sessionId: string;
  phase: SessionPhase;
  error?: string;
}

export interface GetResumeCommandResponse {
  success: boolean;
  harness: string;
  harnessSessionId?: string;
  command: string;
  args: string[];
  cliString: string;
  cwd: string;
  error?: string;
}

export interface SetSessionPhaseResponse {
  success: boolean;
  error?: string;
}

// ============================================================================
// Transcripts — 4-category normalized history
// ============================================================================

export type TranscriptCategory =
  | "user"
  | "assistant"
  | "thinking"
  | "tool_use"
  | "tool_result"
  | "system";

export interface NormalizedTranscriptMessage {
  role: "user" | "assistant" | "system" | "tool";
  category?: TranscriptCategory;
  text: string;
  thinking?: string;
  toolName?: string;
  toolUse?: ToolUsePayload;
  toolResult?: ToolResultPayload;
  timestamp?: string;
}

export interface GetTranscriptRequest {
  harness: string;
  harnessSessionId: string;
  projectRoot?: string;
  transcriptPath?: string;
  maxMessages?: number;
  /** Paginate: return only messages with index < before. */
  before?: number;
}

export interface GetTranscriptResponse {
  harness: string;
  harnessSessionId: string;
  transcriptPath?: string;
  messages: NormalizedTranscriptMessage[];
  hasMore?: boolean;
  truncated?: boolean;
  error?: string;
}

// ============================================================================
// Turn stream / cancel
// ============================================================================

export interface TurnStreamRequest {
  projectRoot?: string;
  sessionId?: string;
  harness: string;
  model?: string;
  prompt: string;
  source?: string;
  streaming?: boolean;
}

export interface TurnStreamAccepted {
  turnId: string;
  sessionId?: string;
  /** SSE endpoint carrying OrchestratorEvent frames for this turn. */
  streamPath: string;
}

export interface TurnCancelRequest {
  turnId: string;
  sessionId?: string;
}

export interface TurnCancelResponse {
  success: boolean;
  message?: string;
  error?: string;
}

// ============================================================================
// Plan / board / dispatch / debate
// ============================================================================

export interface BoardTaskItem {
  id: string;
  subject: string;
  status: string;
  assigneeRole?: string;
  difficulty?: string;
  turn?: number;
  reviewVerdict?: string;
  summary?: string;
}

export interface GetBoardRequest {
  projectRoot?: string;
  sessionId?: string;
}

export interface GetBoardResponse {
  sessionId?: string;
  phase?: string;
  progress?: string;
  tasks: BoardTaskItem[];
}

export interface PlanSyncRequest {
  projectRoot?: string;
  sessionId?: string;
  planMdPath?: string;
}

export interface PlanSyncResponse {
  success: boolean;
  sessionId?: string;
  tasksUpdated?: number;
  stateVersion?: number;
  error?: string;
}

export interface GetPlanResponse {
  sessionId?: string;
  planMdPath: string;
  exists: boolean;
  content: string;
  mtimeMs?: number;
  sha256?: string;
}

export interface SavePlanRequest {
  projectRoot?: string;
  sessionId?: string;
  content: string;
  /** Optimistic concurrency token from GetPlanResponse.sha256 */
  expectedSha256?: string;
  syncToDb?: boolean;
}

export interface SavePlanResponse {
  success: boolean;
  conflict?: boolean;
  sha256?: string;
  mtimeMs?: number;
  tasksUpdated?: number;
  error?: string;
}

export interface DispatchRequest {
  projectRoot?: string;
  sessionId: string;
  turn?: number;
  mode?: "parallel_spawn" | "claim_only";
  taskIds?: string[];
}

export interface DispatchResponse {
  success: boolean;
  dispatched?: string[];
  message?: string;
  error?: string;
}

export interface DebateRequest {
  projectRoot?: string;
  sessionId: string;
  topic: string;
  proposal: string;
}

export interface DebateResponse {
  success: boolean;
  verdict?: string;
  message?: string;
  error?: string;
}

// ============================================================================
// Worktree diff / approve / reject
// ============================================================================

export interface WorktreeDiffFile {
  path: string;
  status?: string;
  additions?: number;
  deletions?: number;
}

export interface WorktreeDiffRequest {
  projectRoot?: string;
  taskId?: string;
  turn?: number;
  wtPath?: string;
  base?: "HEAD" | "merge-base";
}

export interface WorktreeDiffResponse {
  taskId?: string;
  worktreePath?: string;
  mode?: "working" | "committed";
  baseRef?: string;
  files: WorktreeDiffFile[];
  statusShort?: string;
  diffText?: string;
  error?: string;
}

export interface ApproveTaskRequest {
  projectRoot?: string;
  taskId: string;
  summary?: string;
}

export interface ApproveTaskResponse {
  success: boolean;
  merged?: boolean;
  error?: string;
}

export interface RejectTaskRequest {
  projectRoot?: string;
  taskId: string;
  reason?: string;
}

export interface RejectTaskResponse {
  success: boolean;
  error?: string;
}

// ============================================================================
// Subagents
// ============================================================================

export interface SubagentRecordItem {
  subSessionId: string;
  parentSessionId: string;
  taskId?: string;
  role?: string;
  harness?: string;
  model?: string;
  status: string;
  startTime?: string;
  endTime?: string;
  exitCode?: number;
  logPath?: string;
  promptPath?: string;
}

export interface GetSubagentsRequest {
  projectRoot?: string;
  sessionId?: string;
  taskId?: string;
}

export interface GetSubagentsResponse {
  subagents: SubagentRecordItem[];
}

export interface SubagentLogTailRequest {
  projectRoot?: string;
  subSessionId: string;
  tailLines?: number;
}

export interface SubagentLogTailResponse {
  subSessionId: string;
  lines: string[];
  truncated?: boolean;
  error?: string;
}

export type SubagentControlActionKind = "kill" | "retry" | "fallback" | "custom" | "resume";

export interface SubagentActionRequest {
  projectRoot?: string;
  sessionId: string;
  subSessionId: string;
  action: SubagentControlActionKind;
  harness?: string;
  model?: string;
  reason?: string;
}

export interface SubagentActionResponse {
  success: boolean;
  message?: string;
  error?: string;
}

// ============================================================================
// Terminal open
// ============================================================================

export type TerminalKind = "windows-terminal" | "powershell" | "cmd";

export interface TerminalOpenRequest {
  projectRoot: string;
  sessionId: string;
  harness: string;
  terminal?: TerminalKind;
}

export interface TerminalOpenResponse {
  success: boolean;
  /** Interactive resume command for the harness session (copyable). */
  command?: string;
  args?: string[];
  cliString?: string;
  cwd?: string;
  launched?: boolean;
  error?: string;
}

// ============================================================================
// Harness Slash Commands & Skills Discovery
// ============================================================================

export type HarnessCommandSource = "builtin" | "global" | "project";

export type HarnessCommandKind = "command" | "skill" | "workflow";

export interface HarnessCommandItem {
  /** Normalized command name without leading slash (e.g. "compact", "asd-plan"). */
  name: string;
  description: string;
  harness: string;
  source: HarnessCommandSource;
  kind: HarnessCommandKind;
  path?: string;
}

export interface GetHarnessCommandsRequest {
  harness?: string;
  projectRoot?: string;
}

export interface GetHarnessCommandsResponse {
  commands: HarnessCommandItem[];
}

// ============================================================================
// Workspace Files & Directories Listing (GET /api/fs/list)
// ============================================================================

export type FsListMode = "files" | "dirs";

export interface FsListEntry {
  /** Relative path from projectRoot (for files) or display path (for dirs). */
  path: string;
  /** Basename of the file or directory. */
  name: string;
  isDir: boolean;
}

export interface FsListRequest {
  projectRoot?: string;
  mode?: FsListMode;
  prefix?: string;
  limit?: number;
}

export interface FsListResponse {
  projectRoot: string;
  mode: FsListMode;
  baseDir: string;
  entries: FsListEntry[];
  truncated?: boolean;
}

