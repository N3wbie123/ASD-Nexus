/**
 * ASD v2.6.0 Official SDK Boundary Wrapper (src/server/sdk.ts)
 *
 * Clean-room wrapper re-exporting ONLY documented public symbols from
 * "agent-swarm-director" as defined in EXTERNAL_SDK_REFERENCE.md and
 * SDK_SURFACE_CLEANUP_PLAN.md.
 */

import {
  ASD_VERSION,
  attachDriver,
  buildInteractiveResumeHarnessCommand,
  buildResumeHarnessCommand,
  buildSessionDemotion,
  buildSpawnSpec,
  collectDashboardSnapshot,
  discoverHarnessModels,
  discoverProjectCliSessions,
  generateAsdSessionId,
  getActiveDriver,
  handlePlan,
  isValidAsdSessionId,
  listDrivers,
  listKnownProjects,
  openProjectDb,
  parseDriverStreamLine,
  promoteCliSessionToAsd,
  readHarnessTranscript,
  resolveAsdPaths,
  runDriverTurn,
  runSubagentControlAction,
  saveSessionMemo,
  scanSubagentLogs,
  unlinkHarnessSessionFromAsdSession,
} from "agent-swarm-director";

import type {
  CliRecentSessionEntry,
  DashboardSnapshot,
  DriverRow,
  DriverRunResult,
  DriverStreamEvent,
  DriverStreamEventType,
  HarnessId,
  HarnessModelCatalog,
  HarnessTranscript,
  KnownProjectSummary,
  PromoteCliSessionOptions,
  PromoteCliSessionResult,
  ReadTranscriptOptions,
  SessionRow,
  SqliteDriver,
  SubagentLogRecord,
  TaskRow,
  TranscriptMessage,
  TranscriptMessageCategory,
} from "agent-swarm-director";

// Re-export public values & functions
export {
  ASD_VERSION,
  attachDriver,
  buildInteractiveResumeHarnessCommand,
  buildResumeHarnessCommand,
  buildSessionDemotion,
  buildSpawnSpec,
  collectDashboardSnapshot,
  discoverHarnessModels,
  discoverProjectCliSessions,
  generateAsdSessionId,
  getActiveDriver,
  handlePlan,
  isValidAsdSessionId,
  listDrivers,
  listKnownProjects,
  openProjectDb,
  parseDriverStreamLine,
  promoteCliSessionToAsd,
  readHarnessTranscript,
  resolveAsdPaths,
  runDriverTurn,
  runSubagentControlAction,
  saveSessionMemo,
  scanSubagentLogs,
  unlinkHarnessSessionFromAsdSession,
};

// Re-export public types
export type {
  CliRecentSessionEntry,
  DashboardSnapshot,
  DriverRow,
  DriverRunResult,
  DriverStreamEvent,
  DriverStreamEventType,
  HarnessId,
  HarnessModelCatalog,
  HarnessTranscript,
  KnownProjectSummary,
  PromoteCliSessionOptions,
  PromoteCliSessionResult,
  ReadTranscriptOptions,
  SessionRow,
  SqliteDriver,
  SubagentLogRecord,
  TaskRow,
  TranscriptMessage,
  TranscriptMessageCategory,
};

// ============================================================================
// Safe Helpers & Strict Wrappers
// ============================================================================

/**
 * Options for the strict buildResumeCommand wrapper.
 */
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
 * Strict wrapper for buildResumeHarnessCommand that accepts ONLY an options object.
 */
export function buildResumeCommand(
  options: BuildResumeCommandOptions
): { command: string; args: string[] } {
  return buildResumeHarnessCommand({
    ...options,
    prompt: options.prompt ?? "",
  });
}

/**
 * Scoped helper to execute a callback with a read-only SqliteDriver and
 * guarantee connection closure even if the callback throws.
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
