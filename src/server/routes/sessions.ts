import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import type {
  GetSessionsResponse,
  CreateSessionResponse,
  PromoteSessionResponse,
  DemoteSessionResponse,
  UnlinkSessionResponse,
  DeleteSessionResponse,
  SaveSessionMemoResponse,
  SetForegroundSessionResponse,
  SetSessionPhaseResponse,
  GetResumeCommandResponse,
} from "../../shared/protocol.ts";

export function listSessionsRoute(ctx: RequestContext): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const limitStr = ctx.query.get("limit");
  const limit = limitStr ? Number(limitStr) : 100;
  const syncLocal = ctx.query.get("syncLocalTranscripts") === "true";

  try {
    const rawSessions = ctx.deps.discoverProjectCliSessions(projectRoot, {
      limit,
      syncLocalTranscripts: syncLocal,
      homeDir: ctx.homeDir,
    });
    const activeSessionId = ctx.deps.getForegroundSessionId(projectRoot);

    const resp: GetSessionsResponse = {
      sessions: (rawSessions || []).map((s) => ({
        harness: s.harness,
        harnessSessionId: s.harnessSessionId,
        projectRoot: s.projectRoot,
        updatedAt: s.updatedAt,
        model: s.model || undefined,
        summary: s.summary,
        transcriptPath: s.transcriptPath || undefined,
        asdSessionId: s.asdSessionId ?? null,
        asdPhase: (s as any).asdPhase ?? null,
        isPromotedToAsd: (s as any).isPromotedToAsd ?? Boolean(s.asdSessionId),
      })),
      activeSessionId,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      sessions: [],
      activeSessionId: null,
    } satisfies GetSessionsResponse);
  }
}

export function createSessionRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const harness = ctx.body?.harness || "claude-code";
  const model = ctx.body?.model;
  const memo = ctx.body?.memo || ctx.body?.title || "";
  const makeForeground = Boolean(ctx.body?.makeForeground);

  try {
    const sessionId = ctx.deps.generateAsdSessionId();
    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        db.createOrGetSession({
          sessionId,
          projectRoot,
          mainHarness: harness,
          mainModel: model ?? null,
          memo,
        });
        if (makeForeground) {
          ctx.deps.setForegroundSession({
            db,
            projectRoot,
            sessionId,
          });
        }
      },
      ctx.homeDir
    );

    const resp: CreateSessionResponse = {
      success: true,
      sessionId,
      phase: "planning",
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      sessionId: "",
      phase: "planning",
      error: err instanceof Error ? err.message : String(err),
    } satisfies CreateSessionResponse);
  }
}

export function promoteSessionRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  try {
    const res = ctx.deps.promoteCliSessionToAsd({
      projectRoot,
      harness: ctx.body?.harness,
      harnessSessionId: ctx.body?.harnessSessionId,
      model: ctx.body?.model,
      title: ctx.body?.title,
      memo: ctx.body?.memo,
      transcriptPath: ctx.body?.transcriptPath,
      makeForeground: ctx.body?.makeForeground,
      source: ctx.body?.source || "gui",
      homeDir: ctx.homeDir,
    });
    const resp: PromoteSessionResponse = {
      success: true,
      sessionId: res.sessionId,
      created: res.created,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      sessionId: "",
      created: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies PromoteSessionResponse);
  }
}

export function demoteSessionRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const sessionId = ctx.body?.sessionId;
  try {
    const res = ctx.deps.buildSessionDemotion({
      projectRoot,
      sessionId,
      homeDir: ctx.homeDir,
    });
    const resp: DemoteSessionResponse = {
      success: true,
      cliString: res.cliString,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies DemoteSessionResponse);
  }
}

export function unlinkSessionRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const asdSessionId = ctx.body?.asdSessionId;
  const deleteAsdSession = Boolean(ctx.body?.deleteAsdSession);
  try {
    ctx.deps.unlinkHarnessSessionFromAsdSession({
      projectRoot,
      asdSessionId,
      deleteAsdSession,
      homeDir: ctx.homeDir,
    });
    const resp: UnlinkSessionResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies UnlinkSessionResponse);
  }
}

export function deleteSessionRoute(ctx: RequestContext, paramSessionId?: string): void {
  const projectRoot =
    ctx.body?.projectRoot || ctx.query.get("projectRoot") || ctx.projectRoot;
  const sessionId = paramSessionId || ctx.body?.sessionId;
  if (!sessionId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing sessionId",
    } satisfies DeleteSessionResponse);
    return;
  }

  try {
    ctx.deps.unlinkHarnessSessionFromAsdSession({
      projectRoot,
      asdSessionId: sessionId,
      deleteAsdSession: true,
      homeDir: ctx.homeDir,
    });
    try {
      ctx.deps.withReadOnlyProjectDb(
        projectRoot,
        (db) => {
          db.deleteSession(sessionId);
        },
        ctx.homeDir
      );
    } catch {
      // Ignore if session already removed
    }
    const resp: DeleteSessionResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies DeleteSessionResponse);
  }
}

export function saveMemoRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const sessionId = ctx.body?.sessionId;
  const memo = ctx.body?.memo ?? "";

  if (!sessionId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing sessionId",
    } satisfies SaveSessionMemoResponse);
    return;
  }

  try {
    ctx.deps.saveSessionMemo({
      projectRoot,
      sessionId,
      memo,
      homeDir: ctx.homeDir,
    });
    const resp: SaveSessionMemoResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SaveSessionMemoResponse);
  }
}

export function setForegroundRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const sessionId = ctx.body?.sessionId;

  if (!sessionId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing sessionId",
    } satisfies SetForegroundSessionResponse);
    return;
  }

  try {
    let previousSessionId: string | null = null;
    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        previousSessionId = ctx.deps.getForegroundSessionId(projectRoot);
        ctx.deps.setForegroundSession({
          db,
          projectRoot,
          sessionId,
        });
      },
      ctx.homeDir
    );

    const resp: SetForegroundSessionResponse = {
      success: true,
      previousSessionId,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SetForegroundSessionResponse);
  }
}

export function setPhaseRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const sessionId = ctx.body?.sessionId;
  const phase = ctx.body?.phase;

  if (!sessionId || !phase) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing sessionId or phase",
    } satisfies SetSessionPhaseResponse);
    return;
  }

  try {
    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        db.updateSessionPhase(sessionId, phase);
      },
      ctx.homeDir
    );

    const resp: SetSessionPhaseResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SetSessionPhaseResponse);
  }
}

export function getResumeCommandRoute(ctx: RequestContext, sessionId: string): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  let harness = "claude-code";
  let harnessSessionId = "";
  let model: string | undefined = undefined;

  try {
    ctx.deps.withReadOnlyProjectDb(
      projectRoot,
      (db) => {
        const sess = db.getSession(sessionId);
        const driver = db.getActiveDriver(sessionId);
        if (driver?.harness) harness = driver.harness;
        else if (sess?.main_harness) harness = sess.main_harness;

        if (driver?.harness_session_id) harnessSessionId = driver.harness_session_id;
        else if (sess?.harness_session_id) harnessSessionId = sess.harness_session_id;

        if (driver?.model) model = driver.model;
        else if (sess?.main_model) model = sess.main_model;
      },
      ctx.homeDir
    );
  } catch {
    // If DB lookup fails (e.g. in test mock), proceed with defaults
  }

  try {
    const spec = ctx.deps.buildInteractiveResumeHarnessCommand(
      harness,
      harnessSessionId,
      model
    );
    const resp: GetResumeCommandResponse = {
      success: true,
      harness,
      harnessSessionId,
      command: spec.command,
      args: spec.args,
      cliString: spec.cliString,
      cwd: projectRoot,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      harness,
      harnessSessionId,
      command: "",
      args: [],
      cliString: "",
      cwd: projectRoot,
      error: err instanceof Error ? err.message : String(err),
    } satisfies GetResumeCommandResponse);
  }
}
