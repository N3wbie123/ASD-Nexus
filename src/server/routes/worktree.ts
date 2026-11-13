import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  WorktreeDiffResponse,
  WorktreeDiffFile,
  ApproveTaskRequest,
  ApproveTaskResponse,
  RejectTaskRequest,
  RejectTaskResponse,
} from "../../shared/protocol.ts";

export function worktreeDiffRoute(ctx: RequestContext): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const taskId = ctx.query.get("taskId") || undefined;
  const turnStr = ctx.query.get("turn");
  const turn = turnStr ? Number(turnStr) : undefined;
  const wtPath = ctx.query.get("wtPath") || undefined;
  const base = (ctx.query.get("base") as "HEAD" | "merge-base") || undefined;

  try {
    const raw = ctx.deps.getWorktreeDiff(projectRoot, {
      taskId,
      turn,
      wtPath,
      base,
    });

    const rawFiles: unknown[] = raw?.changedFiles || raw?.files || [];
    const files: WorktreeDiffFile[] = rawFiles.map((f: any) => {
      if (typeof f === "string") {
        return { path: f };
      }
      return {
        path: f.path || String(f),
        status: f.status,
        additions: f.additions,
        deletions: f.deletions,
      };
    });

    const resp: WorktreeDiffResponse = {
      taskId,
      worktreePath: raw?.targetDir || raw?.worktreePath,
      mode: raw?.mode,
      baseRef: raw?.baseRef,
      files,
      statusShort: raw?.statusShort,
      diffText: raw?.diff || raw?.diffText,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export async function approveTaskRoute(ctx: RequestContext): Promise<void> {
  const body: ApproveTaskRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;
  const taskId = body.taskId;

  if (!taskId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing taskId",
    } satisfies ApproveTaskResponse);
    return;
  }

  try {
    let merged = true;
    await ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      async (db) => {
        const allTasks = db.getAllTasks();
        const task = allTasks.find((t) => t.task_id === taskId);
        if (!task) {
          throw new Error(`Task not found: ${taskId}`);
        }
        const res = await ctx.deps.approvePendingTask({
          projectRoot,
          sessionId: task.session_id,
          taskId,
          db,
          summary: body.summary,
        });
        merged = res?.merged ?? true;
      },
      ctx.homeDir
    );

    const resp: ApproveTaskResponse = { success: true, merged };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies ApproveTaskResponse);
  }
}

export function rejectTaskRoute(ctx: RequestContext): void {
  const body: RejectTaskRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;
  const taskId = body.taskId;

  if (!taskId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing taskId",
    } satisfies RejectTaskResponse);
    return;
  }

  try {
    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        const allTasks = db.getAllTasks();
        const task = allTasks.find((t) => t.task_id === taskId);
        if (!task) {
          throw new Error(`Task not found: ${taskId}`);
        }
        ctx.deps.rejectPendingTask({
          projectRoot,
          sessionId: task.session_id,
          taskId,
          db,
          reason: body.reason,
        });
      },
      ctx.homeDir
    );

    const resp: RejectTaskResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies RejectTaskResponse);
  }
}
