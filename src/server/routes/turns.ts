import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import type { HarnessId, SqliteDriver } from "../sdk.ts";
import {
  createHarnessStreamNormalizer,
  normalizeDriverStreamEvent,
} from "../normalizer.ts";
import { resolveSlashCommandPromptForTurn } from "./commands.ts";
import type {
  TurnStreamRequest,
  TurnStreamAccepted,
  TurnCancelRequest,
  TurnCancelResponse,
  OrchestratorEvent,
} from "../../shared/protocol.ts";

interface ActiveTurn {
  turnId: string;
  sessionId?: string;
  harness?: string;
  abortController: AbortController;
  listeners: Array<(evt: OrchestratorEvent) => void>;
  ended: boolean;
}

const activeTurns = new Map<string, ActiveTurn>();

export function getActiveTurnCount(): number {
  return activeTurns.size;
}

export function cancelAllActiveTurns(): void {
  for (const turn of activeTurns.values()) {
    if (!turn.abortController.signal.aborted) {
      turn.abortController.abort();
    }
  }
  activeTurns.clear();
}

function setupTurnStream(
  ctx: RequestContext,
  turn: ActiveTurn,
  body: TurnStreamRequest
): void {
  ctx.res.writeHead(200, {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-cache",
    "Connection": "keep-alive",
    "Access-Control-Allow-Origin": "*",
  });

  const writeEvt = (evt: OrchestratorEvent) => {
    if (ctx.res.writableEnded) return;
    ctx.res.write(`data: ${JSON.stringify(evt)}\n\n`);
  };

  turn.listeners.push(writeEvt);

  ctx.req.on("close", () => {
    const idx = turn.listeners.indexOf(writeEvt);
    if (idx !== -1) {
      turn.listeners.splice(idx, 1);
    }
    if (turn.listeners.length === 0 && !turn.abortController.signal.aborted && !turn.ended) {
      turn.abortController.abort();
      activeTurns.delete(turn.turnId);
    }
  });
}

function broadcastEvent(turn: ActiveTurn, evt: OrchestratorEvent): void {
  for (const listener of turn.listeners) {
    try {
      listener(evt);
    } catch {
      // Ignore write errors to closed client sockets
    }
  }
}

export async function turnStreamRoute(ctx: RequestContext): Promise<void> {
  const body: TurnStreamRequest = ctx.body || {};
  const turnId = `turn_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
  const abortController = new AbortController();

  const turn: ActiveTurn = {
    turnId,
    sessionId: body.sessionId,
    harness: body.harness,
    abortController,
    listeners: [],
    ended: false,
  };
  activeTurns.set(turnId, turn);

  const acceptHeader = ctx.req.headers["accept"] || "";
  if (acceptHeader === "application/json") {
    const accepted: TurnStreamAccepted = {
      turnId,
      sessionId: body.sessionId,
      streamPath: `/api/turn/stream/${turnId}`,
    };
    sendJson(ctx.res, 202, accepted);
    // Background execution begins; listener can attach via GET /api/turn/stream/:turnId
    executeTurnInBackground(ctx, turn, body);
    return;
  }

  // Stream directly on the POST request
  setupTurnStream(ctx, turn, body);

  await executeTurn(ctx, turn, body);
}

export function turnStreamGetRoute(ctx: RequestContext, turnId: string): void {
  const turn = activeTurns.get(turnId);
  if (!turn || turn.ended) {
    sendJson(ctx.res, 404, {
      error: `Turn ${turnId} not found or already finished`,
    });
    return;
  }

  setupTurnStream(ctx, turn, {
    harness: turn.harness || "claude-code",
    sessionId: turn.sessionId,
    prompt: "",
  });
}

interface PreparedTurnContext {
  db: SqliteDriver;
  asdSessionId: string;
  harness: HarnessId;
  initialHarnessSessionId?: string;
}

function prepareTurnSessionAndDriver(
  ctx: RequestContext,
  projectRoot: string,
  body: TurnStreamRequest
): PreparedTurnContext {
  const db = ctx.deps.openProjectDb(projectRoot, ctx.homeDir);
  try {
    const requestedHarness = (body.harness || "claude-code") as HarnessId;
    const requestedModel = body.model?.trim() ? body.model.trim() : undefined;
    const rawSid = (body.sessionId || "").trim();
    const source = body.source || "user";

    let asdSessionId: string;
    if (!rawSid) {
      asdSessionId = ctx.deps.generateAsdSessionId();
      db.createOrGetSession({
        sessionId: asdSessionId,
        projectRoot,
        mainHarness: requestedHarness,
        mainModel: requestedModel ?? null,
        memo: "",
      });
    } else if (db.getSession(rawSid)) {
      asdSessionId = rawSid;
    } else if (rawSid.startsWith("asd-") && ctx.deps.isValidAsdSessionId(rawSid)) {
      asdSessionId = rawSid;
      db.createOrGetSession({
        sessionId: asdSessionId,
        projectRoot,
        mainHarness: requestedHarness,
        mainModel: requestedModel ?? null,
        memo: "",
      });
    } else {
      const mapped = db.findSessionByHarnessSessionId?.(rawSid, requestedHarness);
      if (mapped) {
        asdSessionId = mapped.session_id;
      } else {
        try {
          const promoted = ctx.deps.promoteCliSessionToAsd({
            db,
            projectRoot,
            harness: requestedHarness,
            harnessSessionId: rawSid,
            model: requestedModel,
            source,
            homeDir: ctx.homeDir,
          });
          asdSessionId = promoted.sessionId;
        } catch {
          asdSessionId = ctx.deps.isValidAsdSessionId(rawSid)
            ? rawSid
            : ctx.deps.generateAsdSessionId();
          db.createOrGetSession({
            sessionId: asdSessionId,
            projectRoot,
            mainHarness: requestedHarness,
            mainModel: requestedModel ?? null,
            memo: "",
          });
        }
      }
    }

    const active = db.getActiveDriver(asdSessionId);
    const effectiveHarness = (body.harness || active?.harness || "claude-code") as HarnessId;
    let initialHarnessSessionId = active?.harness_session_id ?? undefined;

    if (
      !active ||
      active.harness !== effectiveHarness ||
      (requestedModel !== undefined && requestedModel !== (active.model ?? undefined))
    ) {
      const attached = ctx.deps.attachDriver(db, {
        projectRoot,
        sessionId: asdSessionId,
        harness: effectiveHarness,
        model:
          requestedModel ??
          (active?.harness === effectiveHarness ? active.model : null) ??
          null,
        harnessSessionId:
          active && active.harness === effectiveHarness
            ? active.harness_session_id ?? null
            : null,
        source,
      });
      initialHarnessSessionId = attached?.driver?.harness_session_id ?? initialHarnessSessionId;
    }

    return {
      db,
      asdSessionId,
      harness: effectiveHarness,
      initialHarnessSessionId,
    };
  } catch (err) {
    try {
      db.close();
    } catch {
      // best-effort close on setup failure
    }
    throw err;
  }
}

async function executeTurn(
  ctx: RequestContext,
  turn: ActiveTurn,
  body: TurnStreamRequest
): Promise<void> {
  const projectRoot = body.projectRoot || ctx.projectRoot;
  let prepared: PreparedTurnContext | null = null;
  let dbClosed = false;
  const closeDb = () => {
    if (prepared && !dbClosed) {
      dbClosed = true;
      try {
        prepared.db.close();
      } catch {
        // best-effort DB close
      }
    }
  };

  try {
    prepared = prepareTurnSessionAndDriver(ctx, projectRoot, body);
    turn.sessionId = prepared.asdSessionId;
    turn.harness = prepared.harness;

    const initialEvent: OrchestratorEvent = {
      kind: "status",
      status: "running",
      turnId: turn.turnId,
      sessionId: prepared.asdSessionId,
      harness: prepared.harness,
      timestamp: new Date().toISOString(),
    };
    broadcastEvent(turn, initialEvent);

    const rawNormalizer = createHarnessStreamNormalizer({
      harness: prepared.harness,
      turnId: turn.turnId,
    });
    let sawRawOutput = false;
    let discoveredHarnessSessionId = prepared.initialHarnessSessionId;
    let sawErrorEvent = false;
    let sawInterruptedStatus = false;

    const forwardNormalizedEvents = (events: OrchestratorEvent[]) => {
      for (const evt of events) {
        if (evt.sessionId) {
          discoveredHarnessSessionId = evt.sessionId;
        }
        if (evt.kind === "error") {
          sawErrorEvent = true;
        }
        if (evt.kind === "status") {
          if (evt.status === "interrupted") {
            sawInterruptedStatus = true;
          }
          // Hold terminal done/interrupted until runDriverTurn settles and SQLite closes.
          if (evt.status === "done" || evt.status === "interrupted") {
            continue;
          }
        }
        broadcastEvent(turn, evt);
      }
    };

    const resolvedSlash = resolveSlashCommandPromptForTurn({
      prompt: body.prompt || "",
      harness: prepared.harness,
      projectRoot,
      homeDir: ctx.homeDir,
    });

    const result = await ctx.deps.runDriverTurn({
      projectRoot,
      db: prepared.db,
      sessionId: prepared.asdSessionId,
      prompt: resolvedSlash.prompt,
      model: body.model,
      streamJson: body.streaming ?? true,
      signal: turn.abortController.signal,
      homeDir: ctx.homeDir,
      onOutput: (chunk: string) => {
        sawRawOutput = true;
        forwardNormalizedEvents(rawNormalizer.pushChunk(chunk));
      },
      onEvent: (driverEvent: any) => {
        if (driverEvent?.harnessSessionId) {
          discoveredHarnessSessionId = driverEvent.harnessSessionId;
        }
        if (sawRawOutput) {
          return;
        }
        const norm = normalizeDriverStreamEvent(driverEvent, { turnId: turn.turnId });
        if (norm) {
          if (norm.kind === "error") {
            sawErrorEvent = true;
          }
          broadcastEvent(turn, norm);
        }
      },
    });

    if (sawRawOutput) {
      forwardNormalizedEvents(rawNormalizer.flush());
    }
    if (result?.harnessSessionId) {
      discoveredHarnessSessionId = result.harnessSessionId;
    }

    closeDb();

    if (!turn.abortController.signal.aborted) {
      const nonZeroExit =
        result && typeof result.exitCode === "number" && result.exitCode !== 0;
      const failed = Boolean(nonZeroExit || sawInterruptedStatus);
      if (failed && !sawErrorEvent) {
        broadcastEvent(turn, {
          kind: "error",
          error: `Harness exited with code ${result?.exitCode ?? 1}`,
          turnId: turn.turnId,
          sessionId: prepared.asdSessionId,
          harness: prepared.harness,
          timestamp: new Date().toISOString(),
        });
      }
      broadcastEvent(turn, {
        kind: "status",
        status: failed ? "interrupted" : "done",
        turnId: turn.turnId,
        sessionId: prepared.asdSessionId,
        harness: prepared.harness,
        ...(discoveredHarnessSessionId
          ? {
              raw: {
                asdSessionId: prepared.asdSessionId,
                harnessSessionId: discoveredHarnessSessionId,
              },
            }
          : {}),
        timestamp: new Date().toISOString(),
      });
    }
  } catch (err: unknown) {
    closeDb();
    if (turn.abortController.signal.aborted) {
      broadcastEvent(turn, {
        kind: "status",
        status: "interrupted",
        turnId: turn.turnId,
        sessionId: turn.sessionId || body.sessionId,
        harness: turn.harness || body.harness,
        error: "Turn cancelled",
        timestamp: new Date().toISOString(),
      });
    } else {
      broadcastEvent(turn, {
        kind: "error",
        error: err instanceof Error ? err.message : String(err),
        turnId: turn.turnId,
        sessionId: turn.sessionId || body.sessionId,
        harness: turn.harness || body.harness,
        timestamp: new Date().toISOString(),
      });
      broadcastEvent(turn, {
        kind: "status",
        status: "interrupted",
        turnId: turn.turnId,
        sessionId: turn.sessionId || body.sessionId,
        harness: turn.harness || body.harness,
        timestamp: new Date().toISOString(),
      });
    }
  } finally {
    closeDb();
    turn.ended = true;
    activeTurns.delete(turn.turnId);
    if (!ctx.res.writableEnded) {
      ctx.res.end();
    }
  }
}

function executeTurnInBackground(
  ctx: RequestContext,
  turn: ActiveTurn,
  body: TurnStreamRequest
): void {
  void executeTurn(ctx, turn, body);
}

export function turnCancelRoute(ctx: RequestContext): void {
  const body: TurnCancelRequest = ctx.body || {};
  const turnId = body.turnId;

  if (!turnId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing turnId",
    } satisfies TurnCancelResponse);
    return;
  }

  const turn = activeTurns.get(turnId);
  if (turn) {
    turn.abortController.abort();
    broadcastEvent(turn, {
      kind: "status",
      status: "interrupted",
      turnId,
      sessionId: turn.sessionId,
      harness: turn.harness,
      error: "Turn cancelled",
      timestamp: new Date().toISOString(),
    });
    activeTurns.delete(turnId);
    sendJson(ctx.res, 200, {
      success: true,
      message: "Turn cancelled successfully",
    } satisfies TurnCancelResponse);
    return;
  }

  sendJson(ctx.res, 200, {
    success: true,
    message: "Turn was not active or already finished",
  } satisfies TurnCancelResponse);
}
