import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import type {
  AcpRequestPermissionParams,
  AcpRequestPermissionResult,
  DriverStreamEvent,
  HarnessId,
  SqliteDriver,
} from "../sdk.ts";
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
  TurnPermissionDecisionRequest,
  TurnPermissionDecisionResponse,
  PermissionOptionItem,
  PermissionRequestPayload,
  OrchestratorEvent,
} from "../../shared/protocol.ts";

type PermissionDecision =
  | { outcome: "selected"; optionId: string }
  | { outcome: "cancelled" };

interface PendingPermissionEntry {
  options: PermissionOptionItem[];
  resolve: (decision: PermissionDecision) => void;
}

interface ActiveTurn {
  turnId: string;
  sessionId?: string;
  harness?: string;
  projectRoot?: string;
  autoApprove: boolean;
  abortController: AbortController;
  listeners: Array<(evt: OrchestratorEvent) => void>;
  pendingPermissions: Map<string, PendingPermissionEntry>;
  ended: boolean;
}

const activeTurns = new Map<string, ActiveTurn>();

function cancelPendingPermissions(turn: ActiveTurn): void {
  for (const pending of turn.pendingPermissions.values()) {
    try {
      pending.resolve({ outcome: "cancelled" });
    } catch {
      // ignore settlement errors
    }
  }
  turn.pendingPermissions.clear();
}

export function getActiveTurnCount(): number {
  return activeTurns.size;
}

export function cancelAllActiveTurns(): void {
  for (const turn of activeTurns.values()) {
    cancelPendingPermissions(turn);
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
      cancelPendingPermissions(turn);
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
    projectRoot: body.projectRoot || ctx.projectRoot,
    autoApprove: true,
    abortController,
    listeners: [],
    pendingPermissions: new Map(),
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
  model: string | null;
  initialHarnessSessionId?: string;
  autoApprove: boolean;
  projectRoot: string;
}

function prepareTurnSessionAndDriver(
  ctx: RequestContext,
  projectRoot: string,
  body: TurnStreamRequest
): PreparedTurnContext {
  const db = ctx.deps.openProjectDb(projectRoot, ctx.homeDir);
  try {
    const requestedHarness = (body.harness || "claude-code") as HarnessId;
    const hasModelParam = body.model !== undefined;
    const trimmedModel = typeof body.model === "string" ? body.model.trim() : "";
    const isExplicitModelReset =
      hasModelParam &&
      (trimmedModel === "" || trimmedModel.toLowerCase() === "default");
    const requestedModel =
      hasModelParam && !isExplicitModelReset ? trimmedModel : undefined;
    const rawSid = (body.sessionId || "").trim();
    const source = body.source || "user";

    let asdSessionId: string;
    let existingSessionRow: any = null;
    if (!rawSid) {
      asdSessionId = ctx.deps.generateAsdSessionId();
      db.createOrGetSession({
        sessionId: asdSessionId,
        projectRoot,
        mainHarness: requestedHarness,
        mainModel: isExplicitModelReset ? null : (requestedModel ?? null),
        memo: "",
      });
    } else if ((existingSessionRow = db.getSession(rawSid))) {
      asdSessionId = rawSid;
    } else if (rawSid.startsWith("asd-") && ctx.deps.isValidAsdSessionId(rawSid)) {
      asdSessionId = rawSid;
      db.createOrGetSession({
        sessionId: asdSessionId,
        projectRoot,
        mainHarness: requestedHarness,
        mainModel: isExplicitModelReset ? null : (requestedModel ?? null),
        memo: "",
      });
    } else {
      const mapped = db.findSessionByHarnessSessionId?.(rawSid, requestedHarness);
      if (mapped) {
        asdSessionId = mapped.session_id;
        existingSessionRow = mapped;
      } else {
        const crossHarness =
          db
            .listSessions?.()
            ?.find(
              (s) =>
                s.harness_session_id === rawSid &&
                Boolean(s.main_harness) &&
                s.main_harness !== requestedHarness
            ) ?? db.findSessionByHarnessSessionId?.(rawSid);
        const requestedTrimmed = body.harness?.trim();
        if (
          crossHarness &&
          crossHarness.main_harness &&
          requestedTrimmed &&
          requestedTrimmed !== crossHarness.main_harness
        ) {
          throw new Error(
            `HARNESS_IMMUTABLE: Session ${crossHarness.session_id} (harnessSessionId=${rawSid}) is locked to harness '${crossHarness.main_harness}' and cannot switch to '${requestedTrimmed}'`
          );
        }
        if (crossHarness && !requestedTrimmed) {
          asdSessionId = crossHarness.session_id;
          existingSessionRow = crossHarness;
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
            if (!promoted.created) {
              existingSessionRow = db.getSession(asdSessionId) ?? null;
            }
          } catch {
            asdSessionId = ctx.deps.isValidAsdSessionId(rawSid)
              ? rawSid
              : ctx.deps.generateAsdSessionId();
            db.createOrGetSession({
              sessionId: asdSessionId,
              projectRoot,
              mainHarness: requestedHarness,
              mainModel: isExplicitModelReset ? null : (requestedModel ?? null),
              memo: "",
            });
          }
        }
      }
    }

    const active = db.getActiveDriver(asdSessionId);
    const lockedHarness = (active?.harness || existingSessionRow?.main_harness) as
      | HarnessId
      | undefined;
    const isExistingOrStarted = Boolean(active || existingSessionRow);

    let effectiveHarness: HarnessId;
    if (isExistingOrStarted && lockedHarness) {
      const requestedTrimmed = body.harness?.trim();
      if (requestedTrimmed && requestedTrimmed !== lockedHarness) {
        throw new Error(
          `HARNESS_IMMUTABLE: Session ${asdSessionId} is locked to harness '${lockedHarness}' and cannot switch to '${requestedTrimmed}'`
        );
      }
      effectiveHarness = lockedHarness;
    } else {
      effectiveHarness = (body.harness?.trim() || active?.harness || "claude-code") as HarnessId;
    }

    let initialHarnessSessionId =
      active?.harness_session_id ?? existingSessionRow?.harness_session_id ?? undefined;

    const currentModel =
      (active?.harness === effectiveHarness ? active.model : undefined) ??
      existingSessionRow?.main_model ??
      null;
    const effectiveModel: string | null = isExplicitModelReset
      ? null
      : requestedModel ?? currentModel;
    const modelChanged = isExplicitModelReset
      ? (active?.model ?? null) !== null
      : requestedModel !== undefined && requestedModel !== (active?.model ?? undefined);

    if (!active || active.harness !== effectiveHarness || modelChanged) {
      const preservedHarnessSessionId =
        active && active.harness === effectiveHarness
          ? active.harness_session_id ?? existingSessionRow?.harness_session_id ?? null
          : isExistingOrStarted
            ? existingSessionRow?.harness_session_id ?? null
            : null;
      const attached = ctx.deps.attachDriver(db, {
        projectRoot,
        sessionId: asdSessionId,
        harness: effectiveHarness,
        model: effectiveModel,
        harnessSessionId: preservedHarnessSessionId,
        source,
      });
      initialHarnessSessionId = attached?.driver?.harness_session_id ?? initialHarnessSessionId;
    }

    const sessionRow = db.getSession?.(asdSessionId) ?? existingSessionRow;
    const autoApprove =
      sessionRow?.auto_approve !== undefined
        ? sessionRow.auto_approve !== 0
        : typeof sessionRow?.autoApprove === "boolean"
          ? sessionRow.autoApprove
          : true;

    return {
      db,
      asdSessionId,
      harness: effectiveHarness,
      model: effectiveModel,
      initialHarnessSessionId,
      autoApprove,
      projectRoot,
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

function pickAutoApproveOptionId(options: PermissionOptionItem[]): string | undefined {
  const allowOnce = options.find((opt) => opt.kind === "allow_once");
  if (allowOnce) return allowOnce.optionId;
  const allowAlways = options.find((opt) => opt.kind === "allow_always");
  if (allowAlways) return allowAlways.optionId;
  const anyAllow = options.find((opt) => opt.kind.startsWith("allow"));
  if (anyAllow) return anyAllow.optionId;
  return options[0]?.optionId;
}

function normalizePermissionParams(
  acpParams: any
): Omit<PermissionRequestPayload, "requestId"> & { requestId?: string } {
  const rawInput = acpParams?.toolCall?.rawInput ?? acpParams?.toolInput;
  const toolInput =
    rawInput === undefined
      ? undefined
      : typeof rawInput === "string"
        ? rawInput
        : JSON.stringify(rawInput);

  const rawLocations = acpParams?.toolCall?.locations ?? acpParams?.locations;
  const locations = Array.isArray(rawLocations)
    ? rawLocations
        .map((loc: any) => (typeof loc === "string" ? loc : loc?.path))
        .filter((p: unknown): p is string => typeof p === "string" && p.length > 0)
    : undefined;

  const rawOptions = Array.isArray(acpParams?.options) ? acpParams.options : [];
  const options: PermissionOptionItem[] = rawOptions
    .filter((opt: any) => opt && typeof opt.optionId === "string")
    .map((opt: any) => ({
      optionId: String(opt.optionId),
      name: typeof opt.name === "string" ? opt.name : String(opt.optionId),
      kind: typeof opt.kind === "string" ? opt.kind : "allow_once",
    }));

  const toolKind = acpParams?.toolCall?.kind || acpParams?.toolKind;
  return {
    ...(typeof acpParams?.requestId === "string" && acpParams.requestId
      ? { requestId: acpParams.requestId }
      : {}),
    toolName:
      acpParams?.toolCall?.title ||
      acpParams?.toolCall?.kind ||
      acpParams?.toolName ||
      "tool",
    ...(typeof toolKind === "string" && toolKind ? { toolKind } : {}),
    ...(toolInput !== undefined ? { toolInput } : {}),
    ...(locations && locations.length > 0 ? { locations } : {}),
    options,
  };
}

export function requestTurnPermission(
  turnId: string,
  input: Omit<PermissionRequestPayload, "requestId"> & { requestId?: string }
): Promise<PermissionDecision> {
  const turn = activeTurns.get(turnId);
  if (!turn || turn.ended || turn.abortController.signal.aborted) {
    return Promise.resolve({ outcome: "cancelled" });
  }

  if (turn.autoApprove) {
    const optionId = pickAutoApproveOptionId(input.options || []);
    if (optionId) {
      return Promise.resolve({ outcome: "selected", optionId });
    }
    return Promise.resolve({ outcome: "cancelled" });
  }

  const requestId =
    input.requestId || `perm_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
  const payload: PermissionRequestPayload = {
    requestId,
    toolName: input.toolName,
    ...(input.toolKind !== undefined ? { toolKind: input.toolKind } : {}),
    ...(input.toolInput !== undefined ? { toolInput: input.toolInput } : {}),
    ...(input.locations !== undefined ? { locations: input.locations } : {}),
    options: input.options,
  };

  return new Promise<PermissionDecision>((resolve) => {
    turn.pendingPermissions.set(requestId, {
      options: input.options,
      resolve,
    });
    broadcastEvent(turn, {
      kind: "permission_request",
      turnId: turn.turnId,
      sessionId: turn.sessionId,
      harness: turn.harness,
      permissionRequest: payload,
      timestamp: new Date().toISOString(),
    });
  });
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
    turn.projectRoot = prepared.projectRoot;
    turn.autoApprove = prepared.autoApprove;

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
      model: prepared.model ?? undefined,
      streamJson: body.streaming ?? true,
      autoApprove: prepared.autoApprove,
      signal: turn.abortController.signal,
      homeDir: ctx.homeDir,
      onPermissionRequest: async (
        acpParams: AcpRequestPermissionParams
      ): Promise<AcpRequestPermissionResult> => {
        const decision = await requestTurnPermission(
          turn.turnId,
          normalizePermissionParams(acpParams)
        );
        return { outcome: decision };
      },
      onOutput: (chunk: string) => {
        sawRawOutput = true;
        forwardNormalizedEvents(rawNormalizer.pushChunk(chunk));
      },
      onEvent: (driverEvent: DriverStreamEvent) => {
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
    cancelPendingPermissions(turn);
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
    cancelPendingPermissions(turn);
    if (turn.sessionId) {
      ctx.deps.cancelDriverTurn?.(turn.sessionId);
    }
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

  if (body.sessionId) {
    ctx.deps.cancelDriverTurn?.(body.sessionId);
  }

  sendJson(ctx.res, 200, {
    success: true,
    message: "Turn was not active or already finished",
  } satisfies TurnCancelResponse);
}

export function turnPermissionRoute(ctx: RequestContext): void {
  const body = (ctx.body || {}) as Partial<TurnPermissionDecisionRequest>;
  const turnId = typeof body.turnId === "string" ? body.turnId.trim() : "";
  const requestId = typeof body.requestId === "string" ? body.requestId.trim() : "";
  const outcome = body.outcome;

  if (!turnId || !requestId) {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Missing turnId or requestId",
    } satisfies TurnPermissionDecisionResponse);
    return;
  }

  if (outcome !== "selected" && outcome !== "cancelled") {
    sendJson(ctx.res, 200, {
      success: false,
      error: "Invalid permission outcome",
    } satisfies TurnPermissionDecisionResponse);
    return;
  }

  const turn = activeTurns.get(turnId);
  if (!turn || turn.ended) {
    sendJson(ctx.res, 200, {
      success: false,
      error: `Active turn not found: ${turnId}`,
    } satisfies TurnPermissionDecisionResponse);
    return;
  }

  const pending = turn.pendingPermissions.get(requestId);
  if (!pending) {
    sendJson(ctx.res, 200, {
      success: false,
      error: `Pending permission request not found: ${requestId}`,
    } satisfies TurnPermissionDecisionResponse);
    return;
  }

  if (outcome === "selected") {
    const optionId = typeof body.optionId === "string" ? body.optionId.trim() : "";
    if (!optionId) {
      sendJson(ctx.res, 200, {
        success: false,
        error: "Missing optionId for selected outcome",
      } satisfies TurnPermissionDecisionResponse);
      return;
    }

    const chosenOption = pending.options.find((opt) => opt.optionId === optionId);
    if (chosenOption?.kind === "allow_always" || optionId === "allow_always") {
      turn.autoApprove = true;
      if (turn.sessionId && turn.projectRoot) {
        try {
          ctx.deps.withReadOnlyProjectDb(
            turn.projectRoot,
            (db) => {
              if (typeof (db as any).updateSessionHarnessInfo === "function") {
                (db as any).updateSessionHarnessInfo(turn.sessionId, {
                  autoApprove: true,
                });
              }
            },
            ctx.homeDir
          );
        } catch {
          // Best-effort persistence
        }
      }
    }

    turn.pendingPermissions.delete(requestId);
    pending.resolve({ outcome: "selected", optionId });
    sendJson(ctx.res, 200, {
      success: true,
    } satisfies TurnPermissionDecisionResponse);
    return;
  }

  turn.pendingPermissions.delete(requestId);
  pending.resolve({ outcome: "cancelled" });
  sendJson(ctx.res, 200, {
    success: true,
  } satisfies TurnPermissionDecisionResponse);
}
