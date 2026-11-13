import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import { normalizeDriverStreamEvent } from "../normalizer.ts";
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

async function executeTurn(
  ctx: RequestContext,
  turn: ActiveTurn,
  body: TurnStreamRequest
): Promise<void> {
  const initialEvent: OrchestratorEvent = {
    kind: "status",
    status: "running",
    turnId: turn.turnId,
    sessionId: body.sessionId,
    harness: body.harness,
    timestamp: new Date().toISOString(),
  };
  broadcastEvent(turn, initialEvent);

  try {
    await ctx.deps.runDriverTurn({
      projectRoot: body.projectRoot || ctx.projectRoot,
      sessionId: body.sessionId,
      prompt: body.prompt || "",
      model: body.model,
      streamJson: body.streaming ?? true,
      signal: turn.abortController.signal,
      homeDir: ctx.homeDir,
      onEvent: (driverEvent: any) => {
        const norm = normalizeDriverStreamEvent(driverEvent, { turnId: turn.turnId });
        if (norm) {
          broadcastEvent(turn, norm);
        }
      },
    });

    if (!turn.abortController.signal.aborted) {
      broadcastEvent(turn, {
        kind: "status",
        status: "done",
        turnId: turn.turnId,
        sessionId: body.sessionId,
        harness: body.harness,
        timestamp: new Date().toISOString(),
      });
    }
  } catch (err: unknown) {
    if (turn.abortController.signal.aborted) {
      broadcastEvent(turn, {
        kind: "status",
        status: "interrupted",
        turnId: turn.turnId,
        sessionId: body.sessionId,
        harness: body.harness,
        error: "Turn cancelled",
        timestamp: new Date().toISOString(),
      });
    } else {
      broadcastEvent(turn, {
        kind: "error",
        error: err instanceof Error ? err.message : String(err),
        turnId: turn.turnId,
        sessionId: body.sessionId,
        harness: body.harness,
        timestamp: new Date().toISOString(),
      });
      broadcastEvent(turn, {
        kind: "status",
        status: "interrupted",
        turnId: turn.turnId,
        sessionId: body.sessionId,
        harness: body.harness,
        timestamp: new Date().toISOString(),
      });
    }
  } finally {
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
