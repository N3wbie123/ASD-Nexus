/**
 * Shared SSE & API Event Contracts for ASD-Nexus
 *
 * Clean-room contract for orchestrator streaming events and M1 API DTOs.
 * Pure TypeScript (no Node.js dependencies); safely imported by browser and server.
 */

// ============================================================================
// Normalized Streaming Event Kinds & Statuses
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

// ============================================================================
// Orchestrator Normalized Event Payloads
// ============================================================================

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

export interface OrchestratorEvent {
  kind: OrchestratorEventKind;
  sessionId?: string;
  turnId?: string;
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
// M1 API Request & Response DTOs
// ============================================================================

// 1. /api/turn/stream
export interface TurnStreamRequest {
  projectRoot?: string;
  sessionId?: string;
  harness: string;
  model?: string;
  prompt: string;
  source?: string;
}

// 2. /api/sessions
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

// 3. /api/sessions/promote
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

// 4. /api/transcript/:harness/:id
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
}

export interface GetTranscriptResponse {
  harness: string;
  harnessSessionId: string;
  messages: NormalizedTranscriptMessage[];
  fullCount?: number;
}

// 5. /api/board
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

// 6. /api/subagents
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

// 7. /api/subagent-action
export type SubagentControlActionKind = "kill" | "retry" | "fallback" | "custom";

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
