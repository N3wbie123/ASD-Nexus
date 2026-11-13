import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  GetSubagentsResponse,
  SubagentRecordItem,
  SubagentLogTailResponse,
  SubagentActionRequest,
  SubagentActionResponse,
} from "../../shared/protocol.ts";
import * as fs from "node:fs";
import * as path from "node:path";

export function listSubagentsRoute(ctx: RequestContext): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const filterSessionId = ctx.query.get("sessionId");
  const filterTaskId = ctx.query.get("taskId");

  try {
    const raw = ctx.deps.scanSubagentLogs(projectRoot) || [];
    let filtered = raw;

    if (filterSessionId) {
      filtered = filtered.filter(
        (r) => r.sessionId === filterSessionId
      );
    }
    if (filterTaskId) {
      filtered = filtered.filter((r) => r.taskId === filterTaskId);
    }

    const items: SubagentRecordItem[] = filtered.map((r) => {
      const dir = (r as any).dirPath;
      return {
        subSessionId: r.subSessionId,
        parentSessionId: r.sessionId,
        taskId: r.taskId,
        role: r.role,
        harness: r.harness,
        model: r.model,
        status: r.status,
        startTime: r.startedAt ? new Date(r.startedAt).toISOString() : undefined,
        endTime: r.finishedAt ? new Date(r.finishedAt).toISOString() : undefined,
        exitCode: (r as any).exitCode,
        logPath: dir ? path.join(dir, "output.log") : undefined,
        promptPath: dir ? path.join(dir, "prompt.md") : undefined,
      };
    });

    const resp: GetSubagentsResponse = { subagents: items };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export function subagentLogsRoute(
  ctx: RequestContext,
  paramSubSessionId?: string
): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const subSessionId =
    paramSubSessionId ||
    ctx.query.get("subSessionId") ||
    ctx.body?.subSessionId;
  const tailLinesStr = ctx.query.get("tailLines") || ctx.body?.tailLines;
  const tailLines = tailLinesStr ? Number(tailLinesStr) : 100;

  if (!subSessionId) {
    sendJson(ctx.res, 200, {
      subSessionId: "",
      lines: [],
      error: "Missing subSessionId",
    } satisfies SubagentLogTailResponse);
    return;
  }

  try {
    const raw = ctx.deps.scanSubagentLogs(projectRoot) || [];
    const found = raw.find((r) => r.subSessionId === subSessionId);

    if (!found) {
      sendJson(ctx.res, 200, {
        subSessionId,
        lines: [],
        error: `Subagent not found: ${subSessionId}`,
      } satisfies SubagentLogTailResponse);
      return;
    }

    let logContent = (found as any).outputLog || "";
    const dir = (found as any).dirPath;
    if (dir) {
      const logFile = path.join(dir, "output.log");
      if (fs.existsSync(logFile)) {
        try {
          logContent = fs.readFileSync(logFile, "utf8");
        } catch {}
      }
    }

    const allLines = logContent.split(/\r?\n/);
    const startIdx = Math.max(0, allLines.length - tailLines);
    const lines = allLines.slice(startIdx);
    const truncated = startIdx > 0;

    const resp: SubagentLogTailResponse = {
      subSessionId,
      lines,
      truncated,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      subSessionId,
      lines: [],
      error: err instanceof Error ? err.message : String(err),
    } satisfies SubagentLogTailResponse);
  }
}

export async function subagentActionRoute(ctx: RequestContext): Promise<void> {
  const body: SubagentActionRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;

  try {
    const res = await ctx.deps.runSubagentControlAction({
      projectRoot,
      sessionId: body.sessionId,
      subSessionId: body.subSessionId,
      action: body.action,
      harness: body.harness,
      model: body.model,
      reason: body.reason,
      homeDir: ctx.homeDir,
    });
    const resp: SubagentActionResponse = {
      success: true,
      message: res?.message || `Subagent action ${body.action} succeeded`,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SubagentActionResponse);
  }
}
