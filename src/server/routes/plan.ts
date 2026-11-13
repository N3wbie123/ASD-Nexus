import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  GetBoardResponse,
  BoardTaskItem,
  GetPlanResponse,
  SavePlanRequest,
  SavePlanResponse,
  PlanSyncRequest,
  PlanSyncResponse,
  DispatchRequest,
  DispatchResponse,
  DebateRequest,
  DebateResponse,
} from "../../shared/protocol.ts";
import * as fs from "node:fs";
import * as path from "node:path";
import * as crypto from "node:crypto";

export function defaultReadPlanFile(planPath: string): {
  exists: boolean;
  content: string;
  mtimeMs?: number;
  sha256?: string;
} {
  if (!fs.existsSync(planPath)) {
    return { exists: false, content: "" };
  }
  const content = fs.readFileSync(planPath, "utf8");
  const stats = fs.statSync(planPath);
  const sha256 = crypto.createHash("sha256").update(content).digest("hex");
  return { exists: true, content, mtimeMs: stats.mtimeMs, sha256 };
}

export function defaultWritePlanFile(
  planPath: string,
  content: string
): { sha256: string; mtimeMs: number } {
  fs.mkdirSync(path.dirname(planPath), { recursive: true });
  fs.writeFileSync(planPath, content, "utf8");
  const stats = fs.statSync(planPath);
  const sha256 = crypto.createHash("sha256").update(content).digest("hex");
  return { sha256, mtimeMs: stats.mtimeMs };
}

export async function getBoardRoute(ctx: RequestContext): Promise<void> {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const requestedSessionId = ctx.query.get("sessionId");

  try {
    const snap = await ctx.deps.collectDashboardSnapshot({
      projectRoot,
      homeDir: ctx.homeDir,
    });

    const tasksRaw = snap.tasks || [];
    let doneCount = 0;
    const taskItems: BoardTaskItem[] = [];

    for (const t of tasksRaw) {
      if (t.status === "done") doneCount++;
      let summaryText: string | undefined = (t as any).summary;
      if (!summaryText && (t as any).done_claim_json) {
        try {
          const parsed = JSON.parse((t as any).done_claim_json);
          summaryText = parsed.summary;
        } catch {}
      }
      taskItems.push({
        id: (t as any).task_id || (t as any).id,
        subject: (t as any).title || (t as any).subject || "",
        status: (t as any).status || "pending",
        assigneeRole: (t as any).role || (t as any).assigneeRole,
        difficulty: (t as any).difficulty,
        turn: (t as any).turn,
        reviewVerdict: (t as any).review_verdict || (t as any).reviewVerdict,
        summary: summaryText,
      });
    }

    const totalCount = taskItems.length;
    const progress = totalCount > 0 ? `${doneCount}/${totalCount}` : undefined;
    const activeSessionId =
      requestedSessionId ||
      snap.session?.session_id ||
      ctx.deps.getForegroundSessionId(projectRoot) ||
      undefined;

    const resp: GetBoardResponse = {
      sessionId: activeSessionId,
      phase: snap.session?.phase,
      progress,
      tasks: taskItems,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export function getPlanRoute(ctx: RequestContext): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const requestedSessionId = ctx.query.get("sessionId") || undefined;

  try {
    const paths = ctx.deps.resolveAsdPaths(projectRoot, ctx.homeDir);
    const planPath = paths.planMdPath;

    const reader = ctx.deps.readPlanFile || defaultReadPlanFile;
    const info = reader(planPath);

    const fgSession = ctx.deps.getForegroundSessionId(projectRoot, planPath);
    const sessionId = fgSession || requestedSessionId;

    const resp: GetPlanResponse = {
      sessionId,
      planMdPath: planPath,
      exists: info.exists,
      content: info.content,
      mtimeMs: info.mtimeMs,
      sha256: info.sha256,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export function savePlanRoute(ctx: RequestContext): void {
  const body: SavePlanRequest & { expectedHash?: string; expectedMtimeMs?: number } =
    ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;
  const paths = ctx.deps.resolveAsdPaths(projectRoot, ctx.homeDir);
  const planPath = paths.planMdPath;

  const reader = ctx.deps.readPlanFile || defaultReadPlanFile;
  const writer = ctx.deps.writePlanFile || defaultWritePlanFile;

  const current = reader(planPath);

  // Optimistic concurrency conflict check (409)
  const expectedSha = body.expectedSha256 || body.expectedHash;
  if (expectedSha !== undefined && current.exists && current.sha256 !== expectedSha) {
    sendJson(ctx.res, 409, {
      success: false,
      conflict: true,
      sha256: current.sha256,
      mtimeMs: current.mtimeMs,
      error: "Conflict: PLAN.md has been modified concurrently (SHA256 mismatch)",
    } satisfies SavePlanResponse);
    return;
  }

  if (body.expectedMtimeMs !== undefined && current.exists && current.mtimeMs !== undefined) {
    if (Math.abs(current.mtimeMs - body.expectedMtimeMs) > 1) {
      sendJson(ctx.res, 409, {
        success: false,
        conflict: true,
        sha256: current.sha256,
        mtimeMs: current.mtimeMs,
        error: "Conflict: PLAN.md has been modified concurrently (mtime mismatch)",
      } satisfies SavePlanResponse);
      return;
    }
  }

  try {
    const writeResult = writer(planPath, body.content ?? "");
    let tasksUpdated = 0;

    const syncToDb = body.syncToDb !== false;
    if (syncToDb) {
      const sessionId =
        body.sessionId ||
        ctx.deps.getForegroundSessionId(projectRoot, planPath) ||
        "";
      if (sessionId) {
        ctx.deps.withReadOnlyProjectDb(
          projectRoot,
          (db) => {
            const syncRes = ctx.deps.syncPlanFromMd(db, sessionId, planPath);
            tasksUpdated = syncRes?.tasksUpdated ?? syncRes?.updatedCount ?? 0;
          },
          ctx.homeDir
        );
      }
    }

    const resp: SavePlanResponse = {
      success: true,
      conflict: false,
      sha256: writeResult.sha256,
      mtimeMs: writeResult.mtimeMs,
      tasksUpdated,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SavePlanResponse);
  }
}

export function syncPlanRoute(ctx: RequestContext): void {
  const body: PlanSyncRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;
  const paths = ctx.deps.resolveAsdPaths(projectRoot, ctx.homeDir);
  const planPath = body.planMdPath || paths.planMdPath;
  const sessionId =
    body.sessionId ||
    ctx.deps.getForegroundSessionId(projectRoot, planPath) ||
    "";

  if (!sessionId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing sessionId and no foreground session in PLAN.md",
    } satisfies PlanSyncResponse);
    return;
  }

  try {
    let tasksUpdated = 0;
    let stateVersion: number | undefined = undefined;

    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        const syncRes = ctx.deps.syncPlanFromMd(db, sessionId, planPath);
        tasksUpdated = syncRes?.tasksUpdated ?? syncRes?.updatedCount ?? 0;
        stateVersion = syncRes?.stateVersion;
      },
      ctx.homeDir
    );

    const resp: PlanSyncResponse = {
      success: true,
      sessionId,
      tasksUpdated,
      stateVersion,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies PlanSyncResponse);
  }
}

export async function dispatchRoute(ctx: RequestContext): Promise<void> {
  const body: DispatchRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;

  try {
    const res = await ctx.deps.handleDispatch(
      {
        sessionId: body.sessionId,
        turn: body.turn,
        mode: body.mode,
        taskIds: body.taskIds,
      },
      { projectRoot, homeDir: ctx.homeDir }
    );
    const resp: DispatchResponse = {
      success: true,
      dispatched: res?.dispatched,
      message: res?.message,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies DispatchResponse);
  }
}

export async function debateRoute(ctx: RequestContext): Promise<void> {
  const body: DebateRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;

  try {
    const res = await ctx.deps.handleDebate(
      {
        sessionId: body.sessionId,
        topic: body.topic,
        proposal: body.proposal,
      },
      { projectRoot, homeDir: ctx.homeDir }
    );
    const resp: DebateResponse = {
      success: true,
      verdict: res?.verdict,
      message: res?.message,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies DebateResponse);
  }
}
