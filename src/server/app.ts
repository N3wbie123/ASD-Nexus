import * as http from "node:http";
import { URL } from "node:url";
import * as sdk from "./sdk.ts";
import type { RequestContext } from "./routes/types.ts";
import { sendError } from "./routes/types.ts";
import { getHealthRoute } from "./routes/health.ts";
import {
  listProjectsRoute,
  selectProjectRoute,
  resolveProjectPathsRoute,
} from "./routes/projects.ts";
import { getModelsRoute } from "./routes/models.ts";
import { getRoutingRoute, saveRoutingRoute } from "./routes/routing.ts";
import {
  listSessionsRoute,
  createSessionRoute,
  promoteSessionRoute,
  demoteSessionRoute,
  unlinkSessionRoute,
  deleteSessionRoute,
  saveMemoRoute,
  setForegroundRoute,
  setPhaseRoute,
  getResumeCommandRoute,
} from "./routes/sessions.ts";
import { getTranscriptRoute } from "./routes/transcripts.ts";
import {
  turnStreamRoute,
  turnStreamGetRoute,
  turnCancelRoute,
  cancelAllActiveTurns,
} from "./routes/turns.ts";
import {
  getBoardRoute,
  getPlanRoute,
  savePlanRoute,
  syncPlanRoute,
  dispatchRoute,
  debateRoute,
} from "./routes/plan.ts";
import {
  worktreeDiffRoute,
  approveTaskRoute,
  rejectTaskRoute,
} from "./routes/worktree.ts";
import {
  listSubagentsRoute,
  subagentLogsRoute,
  subagentActionRoute,
} from "./routes/subagents.ts";
import { terminalOpenRoute } from "./routes/terminal.ts";

export interface OrchestratorDeps {
  version: string;
  listKnownProjects: (options?: {
    skipDiskCheck?: boolean;
    forceRefresh?: boolean;
    homeDir?: string;
  }) => sdk.KnownProjectSummary[];
  ensureProjectInitialized: (projectRoot: string, homeDir?: string) => void;
  resolveAsdPaths: (projectRoot: string, homeDir?: string) => sdk.AsdPaths;
  openProjectDb: (projectRoot: string, homeDir?: string) => sdk.SqliteDriver;
  withReadOnlyProjectDb: <T>(
    projectRoot: string,
    fn: (db: sdk.SqliteDriver) => T,
    homeDir?: string
  ) => T;
  discoverHarnessModels: (options?: {
    projectRoot?: string;
    homeDir?: string;
    forceRefresh?: boolean;
  }) => Promise<sdk.HarnessModelCatalog>;
  loadRoutingConfig: (projectRoot: string, homeDir?: string) => sdk.RoutingConfig;
  saveProjectRoutingConfig: (options: {
    projectRoot: string;
    action: "save" | "reset";
    routingConfig?: any;
    homeDir?: string;
  }) => any;
  discoverProjectCliSessions: (
    projectRoot: string,
    options?: {
      limit?: number;
      syncLocalTranscripts?: boolean;
      homeDir?: string;
    }
  ) => sdk.CliRecentSessionEntry[];
  getForegroundSessionId: (projectRoot: string, planMdPath?: string) => string | null;
  setForegroundSession: (input: {
    db: sdk.SqliteDriver;
    projectRoot: string;
    sessionId: string;
    planMdPath?: string;
  }) => any;
  generateAsdSessionId: () => string;
  isValidAsdSessionId: (id: string) => boolean;
  attachDriver: typeof sdk.attachDriver;
  promoteCliSessionToAsd: (
    options: sdk.PromoteCliSessionOptions
  ) => sdk.PromoteCliSessionResult;
  buildSessionDemotion: (options: {
    projectRoot: string;
    sessionId?: string;
    homeDir?: string;
    db?: sdk.SqliteDriver;
  }) => sdk.SessionDemotionResult;
  unlinkHarnessSessionFromAsdSession: (options: {
    projectRoot: string;
    asdSessionId?: string;
    deleteAsdSession?: boolean;
    homeDir?: string;
  }) => any;
  saveSessionMemo: (options: {
    projectRoot: string;
    sessionId: string;
    memo: string;
    homeDir?: string;
  }) => any;
  buildInteractiveResumeHarnessCommand: (
    harnessOrOptions: any,
    harnessSessionId?: string,
    model?: string
  ) => { command: string; args: string[]; cliString: string };
  buildResumeCommand: (
    options: sdk.BuildResumeCommandOptions
  ) => { command: string; args: string[] };
  readHarnessTranscript: (
    harness: any,
    sessionId: string,
    options?: sdk.ReadTranscriptOptions
  ) => sdk.HarnessTranscript;
  runDriverTurn: (input: any) => Promise<sdk.DriverRunResult>;
  collectDashboardSnapshot: (options?: {
    projectRoot?: string;
    homeDir?: string;
  }) => Promise<sdk.DashboardSnapshot>;
  syncPlanFromMd: (
    db: sdk.SqliteDriver,
    sessionId: string,
    planMdPath: string
  ) => any;
  handlePlan: (input: any, options?: any) => Promise<any>;
  handleDispatch: (input: any, options?: any) => Promise<any>;
  handleDebate: (input: any, options?: any) => Promise<any>;
  getWorktreeDiff: (projectRoot: string, options?: any) => any;
  approvePendingTask: (options: any) => Promise<any>;
  rejectPendingTask: (options: any) => any;
  scanSubagentLogs: (projectRoot: string) => sdk.SubagentLogRecord[];
  runSubagentControlAction: (options: any) => Promise<any>;
  readPlanFile?: (planPath: string) => {
    exists: boolean;
    content: string;
    mtimeMs?: number;
    sha256?: string;
  };
  writePlanFile?: (
    planPath: string,
    content: string
  ) => { sha256: string; mtimeMs: number };
}

export function createDefaultDeps(): OrchestratorDeps {
  return {
    version: sdk.ASD_VERSION,
    listKnownProjects: sdk.listKnownProjects,
    ensureProjectInitialized: sdk.ensureProjectInitialized,
    resolveAsdPaths: sdk.resolveAsdPaths,
    openProjectDb: sdk.openProjectDb,
    withReadOnlyProjectDb: sdk.withReadOnlyProjectDb,
    discoverHarnessModels: sdk.discoverHarnessModels,
    loadRoutingConfig: sdk.loadRoutingConfig,
    saveProjectRoutingConfig: sdk.saveProjectRoutingConfig,
    discoverProjectCliSessions: sdk.discoverProjectCliSessions,
    getForegroundSessionId: sdk.getForegroundSessionId,
    setForegroundSession: sdk.setForegroundSession,
    generateAsdSessionId: sdk.generateAsdSessionId,
    isValidAsdSessionId: sdk.isValidAsdSessionId,
    attachDriver: sdk.attachDriver,
    promoteCliSessionToAsd: sdk.promoteCliSessionToAsd,
    buildSessionDemotion: sdk.buildSessionDemotion,
    unlinkHarnessSessionFromAsdSession: sdk.unlinkHarnessSessionFromAsdSession,
    saveSessionMemo: sdk.saveSessionMemo,
    buildInteractiveResumeHarnessCommand: sdk.buildInteractiveResumeHarnessCommand,
    buildResumeCommand: sdk.buildResumeCommand,
    readHarnessTranscript: sdk.readHarnessTranscript,
    runDriverTurn: sdk.runDriverTurn,
    collectDashboardSnapshot: sdk.collectDashboardSnapshot,
    syncPlanFromMd: sdk.syncPlanFromMd,
    handlePlan: sdk.handlePlan,
    handleDispatch: sdk.handleDispatch,
    handleDebate: sdk.handleDebate,
    getWorktreeDiff: sdk.getWorktreeDiff,
    approvePendingTask: sdk.approvePendingTask,
    rejectPendingTask: sdk.rejectPendingTask,
    scanSubagentLogs: sdk.scanSubagentLogs,
    runSubagentControlAction: sdk.runSubagentControlAction,
  };
}

export interface AppOptions {
  token?: string;
  defaultProjectRoot?: string;
  homeDir?: string;
}

function parseRequestBody(req: http.IncomingMessage): Promise<any> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    const MAX_SIZE = 10 * 1024 * 1024;

    req.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_SIZE) {
        req.destroy();
        reject(new Error("Request body too large"));
        return;
      }
      chunks.push(chunk);
    });

    req.on("end", () => {
      if (chunks.length === 0) {
        resolve({});
        return;
      }
      const raw = Buffer.concat(chunks).toString("utf8").trim();
      if (!raw) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(raw));
      } catch (err) {
        reject(err);
      }
    });

    req.on("error", (err) => {
      reject(err);
    });
  });
}

async function matchAndDispatchRoute(ctx: RequestContext): Promise<void> {
  const method = ctx.req.method || "GET";
  const path = ctx.pathname;

  // Exact routes
  if (method === "GET" && path === "/api/health") {
    getHealthRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/projects") {
    listProjectsRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/projects/select") {
    selectProjectRoute(ctx);
    return;
  }
  if ((method === "GET" || method === "POST") && path === "/api/projects/resolve") {
    resolveProjectPathsRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/models") {
    await getModelsRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/models/refresh") {
    await getModelsRoute(ctx, true);
    return;
  }
  if (method === "GET" && path === "/api/routing") {
    getRoutingRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/routing") {
    saveRoutingRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/sessions") {
    listSessionsRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions") {
    createSessionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/promote") {
    promoteSessionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/demote") {
    demoteSessionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/unlink") {
    unlinkSessionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/delete") {
    deleteSessionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/memo") {
    saveMemoRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/foreground") {
    setForegroundRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/sessions/phase") {
    setPhaseRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/turn/stream") {
    await turnStreamRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/turn/cancel") {
    turnCancelRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/board") {
    await getBoardRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/plan") {
    getPlanRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/plan") {
    savePlanRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/plan/sync") {
    syncPlanRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/dispatch") {
    await dispatchRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/debate") {
    await debateRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/worktree/diff") {
    worktreeDiffRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/tasks/approve") {
    await approveTaskRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/tasks/reject") {
    rejectTaskRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/subagents") {
    listSubagentsRoute(ctx);
    return;
  }
  if (method === "GET" && path === "/api/subagents/logs") {
    subagentLogsRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/subagent-action") {
    await subagentActionRoute(ctx);
    return;
  }
  if (method === "POST" && path === "/api/terminal/open") {
    terminalOpenRoute(ctx);
    return;
  }

  // Parameterized routes
  const resumeMatch = path.match(/^\/api\/sessions\/([^/]+)\/resume-command$/);
  if (method === "GET" && resumeMatch?.[1]) {
    getResumeCommandRoute(ctx, decodeURIComponent(resumeMatch[1]));
    return;
  }

  const deleteMatch = path.match(/^\/api\/sessions\/([^/]+)$/);
  if (method === "DELETE" && deleteMatch?.[1]) {
    deleteSessionRoute(ctx, decodeURIComponent(deleteMatch[1]));
    return;
  }

  const transcriptMatch = path.match(/^\/api\/transcript\/([^/]+)\/([^/]+)$/);
  if (method === "GET" && transcriptMatch?.[1] && transcriptMatch?.[2]) {
    getTranscriptRoute(
      ctx,
      decodeURIComponent(transcriptMatch[1]),
      decodeURIComponent(transcriptMatch[2])
    );
    return;
  }

  const turnStreamMatch = path.match(/^\/api\/turn\/stream\/([^/]+)$/);
  if (method === "GET" && turnStreamMatch?.[1]) {
    turnStreamGetRoute(ctx, decodeURIComponent(turnStreamMatch[1]));
    return;
  }

  const subagentLogsMatch = path.match(/^\/api\/subagents\/([^/]+)\/logs$/);
  if (method === "GET" && subagentLogsMatch?.[1]) {
    subagentLogsRoute(ctx, decodeURIComponent(subagentLogsMatch[1]));
    return;
  }

  sendError(ctx.res, 404, `Route not found: ${method} ${path}`);
}

export function createOrchestratorApp(
  deps?: Partial<OrchestratorDeps>,
  options?: AppOptions
): http.Server {
  const fullDeps: OrchestratorDeps = {
    ...createDefaultDeps(),
    ...deps,
  };
  const appOptions: AppOptions = {
    defaultProjectRoot: options?.defaultProjectRoot || process.cwd(),
    ...options,
  };

  const server = http.createServer(async (req, res) => {
    // CORS headers on all responses
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.setHeader(
      "Access-Control-Allow-Methods",
      "GET, POST, PUT, DELETE, OPTIONS"
    );
    res.setHeader(
      "Access-Control-Allow-Headers",
      "Content-Type, Authorization"
    );

    if (req.method === "OPTIONS") {
      res.writeHead(204);
      res.end();
      return;
    }

    // Token auth check if configured
    if (appOptions.token) {
      const authHeader = req.headers["authorization"] || "";
      const isAuthed =
        typeof authHeader === "string" &&
        authHeader.toLowerCase().startsWith("bearer ") &&
        authHeader.slice(7).trim() === appOptions.token;

      if (!isAuthed) {
        sendError(res, 401, "Unauthorized: Invalid or missing bearer token");
        return;
      }
    }

    const hostHeader = req.headers.host || "127.0.0.1";
    const parsedUrl = new URL(req.url || "/", `http://${hostHeader}`);
    const pathname = parsedUrl.pathname.replace(/\/+$/, "") || "/";

    let body: any = {};
    if (
      req.method === "POST" ||
      req.method === "PUT" ||
      req.method === "PATCH" ||
      req.method === "DELETE"
    ) {
      try {
        body = await parseRequestBody(req);
      } catch (err: unknown) {
        sendError(res, 400, "Invalid JSON payload in request body");
        return;
      }
    }

    const projectRoot =
      body?.projectRoot ||
      parsedUrl.searchParams.get("projectRoot") ||
      appOptions.defaultProjectRoot ||
      process.cwd();

    const ctx: RequestContext = {
      req,
      res,
      url: parsedUrl,
      pathname,
      query: parsedUrl.searchParams,
      body,
      deps: fullDeps,
      options: appOptions,
      projectRoot,
      homeDir: appOptions.homeDir,
    };

    try {
      await matchAndDispatchRoute(ctx);
    } catch (err: unknown) {
      if (!res.writableEnded) {
        sendError(res, 500, err instanceof Error ? err.message : String(err));
      }
    }
  });

  server.on("close", () => {
    cancelAllActiveTurns();
  });

  return server;
}
