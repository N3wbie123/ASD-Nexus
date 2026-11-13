/**
 * ASD v2.6.0 Official SDK Boundary (src/server/sdk.ts)
 *
 * Single import point through which the ASD-Nexus sidecar server touches the
 * `agent-swarm-director` runtime. Re-exports ONLY the documented v2.6.0 public
 * surface (EXTERNAL_SDK_REFERENCE.md + SDK_SURFACE_CLEANUP_PLAN.md §3): no
 * dashboard-server entry, no removed aliases, no adapter-method leftovers.
 *
 * Process discipline: harness CLI command assembly happens exclusively through
 * the buildSpawnSpec family below; this module never spawns processes itself.
 */

import {
  ASD_VERSION,
  HARNESS_IDS,
  approvePendingTask,
  attachDriver,
  buildHeadlessHarnessCommand,
  buildInteractiveResumeHarnessCommand,
  buildResumeHarnessCommand,
  buildSessionDemotion,
  buildSpawnSpec,
  collectDashboardSnapshot,
  detachDriver,
  discoverHarnessModels,
  discoverProjectCliSessions,
  ensureProjectInitialized,
  generateAsdSessionId,
  getActiveDriver,
  getForegroundSessionId,
  getWorktreeDiff,
  handleDebate,
  handleDispatch,
  handlePlan,
  handleVerify,
  isValidAsdSessionId,
  listDrivers,
  listKnownProjects,
  loadRoutingConfig,
  locateHarnessTranscript,
  openProjectDb,
  parseDriverStreamLine,
  parsePlanMd,
  promoteCliSessionToAsd,
  readHarnessTranscript,
  rejectPendingTask,
  renderPlanMd,
  resolveAsdPaths,
  runDriverTurn,
  runSubagentControlAction,
  saveProjectRoutingConfig,
  saveSessionMemo,
  scanSubagentLogs,
  setForegroundSession,
  syncPlanFromMd,
  unlinkHarnessSessionFromAsdSession,
} from "agent-swarm-director";

import type {
  AsdPaths,
  CliRecentSessionEntry,
  DashboardSnapshot,
  DriverRow,
  DriverRunResult,
  DriverStreamEvent,
  DriverStreamEventType,
  HarnessId,
  HarnessModelCatalog,
  HarnessTranscript,
  HeadlessHarnessCommandOptions,
  KnownProjectSummary,
  ParsedPlanTask,
  PromoteCliSessionOptions,
  PromoteCliSessionResult,
  ReadTranscriptOptions,
  RoutingConfig,
  SessionDemotionResult,
  SessionRow,
  SqliteDriver,
  SubagentLogRecord,
  TaskRow,
  TranscriptMessage,
  TranscriptMessageCategory,
} from "agent-swarm-director";

// ============================================================================
// Re-exported public values & functions (v2.6.0 surface only)
// ============================================================================

export {
  // version & harness constants
  ASD_VERSION,
  HARNESS_IDS,
  // config & paths
  resolveAsdPaths,
  loadRoutingConfig,
  ensureProjectInitialized,
  // database
  openProjectDb,
  generateAsdSessionId,
  isValidAsdSessionId,
  // session discovery / promotion / demotion / projects
  discoverProjectCliSessions,
  promoteCliSessionToAsd,
  unlinkHarnessSessionFromAsdSession,
  buildSessionDemotion,
  listKnownProjects,
  // harness command assembly (single sanctioned path)
  buildSpawnSpec,
  buildHeadlessHarnessCommand,
  buildResumeHarnessCommand,
  buildInteractiveResumeHarnessCommand,
  // streaming turns & stream parser
  runDriverTurn,
  parseDriverStreamLine,
  // driver registry
  attachDriver,
  detachDriver,
  getActiveDriver,
  listDrivers,
  // transcripts (4-category)
  readHarnessTranscript,
  locateHarnessTranscript,
  // models
  discoverHarnessModels,
  // session services
  saveSessionMemo,
  collectDashboardSnapshot,
  // routing
  saveProjectRoutingConfig,
  // workflow: plan / dispatch / verify / debate
  handlePlan,
  handleDispatch,
  handleVerify,
  handleDebate,
  syncPlanFromMd,
  parsePlanMd,
  renderPlanMd,
  setForegroundSession,
  getForegroundSessionId,
  // worktree diff / task gate
  getWorktreeDiff,
  approvePendingTask,
  rejectPendingTask,
  // subagents
  runSubagentControlAction,
  scanSubagentLogs,
};

// ============================================================================
// Re-exported public types (v2.6.0 surface only)
// ============================================================================

export type {
  AsdPaths,
  CliRecentSessionEntry,
  DashboardSnapshot,
  DriverRow,
  DriverRunResult,
  DriverStreamEvent,
  DriverStreamEventType,
  HarnessId,
  HarnessModelCatalog,
  HarnessTranscript,
  HeadlessHarnessCommandOptions,
  KnownProjectSummary,
  ParsedPlanTask,
  PromoteCliSessionOptions,
  PromoteCliSessionResult,
  ReadTranscriptOptions,
  RoutingConfig,
  SessionDemotionResult,
  SessionRow,
  SqliteDriver,
  SubagentLogRecord,
  TaskRow,
  TranscriptMessage,
  TranscriptMessageCategory,
};

// ============================================================================
// Strict wrappers & scoped helpers
// ============================================================================

/** Options for the strict resume-command wrapper (options object only). */
export interface BuildResumeCommandOptions {
  harness: HarnessId;
  harnessSessionId: string;
  prompt?: string;
  model?: string;
  thinkingEffort?: string | null;
  reasoningEffort?: string | null;
  streaming?: boolean;
}

/**
 * Strict options-object-only wrapper around buildResumeHarnessCommand.
 * The runtime function is options-only in v2.6.0; this wrapper keeps that
 * contract explicit for server call sites.
 */
export function buildResumeCommand(
  options: BuildResumeCommandOptions
): { command: string; args: string[] } {
  return buildResumeHarnessCommand({
    harness: options.harness,
    harnessSessionId: options.harnessSessionId,
    prompt: options.prompt ?? "",
    model: options.model,
    thinkingEffort: options.thinkingEffort,
    reasoningEffort: options.reasoningEffort,
    streaming: options.streaming,
  });
}

/**
 * Executes a callback with a project-scoped SqliteDriver and guarantees the
 * connection is closed even if the callback throws.
 */
export function withReadOnlyProjectDb<T>(
  projectRoot: string,
  fn: (db: SqliteDriver) => T,
  homeDir?: string
): T {
  const db = openProjectDb(projectRoot, homeDir);
  try {
    return fn(db);
  } finally {
    db.close();
  }
}
