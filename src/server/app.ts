/**
 * Orchestrator HTTP + SSE Server (src/server/app.ts)
 *
 * Clean-room `node:http` server exposing M1 endpoints on top of the ASD v2.6.0 SDK:
 * - GET  /api/health
 * - GET  /api/sessions
 * - POST /api/sessions/promote
 * - GET  /api/transcript/:harness/:id
 * - GET  /api/board
 * - GET  /api/subagents
 * - POST /api/subagent-action
 * - POST /api/turn/stream (SSE normalized OrchestratorEvent stream)
 */

import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as http from "node:http";
import * as path from "node:path";

import {
  ASD_VERSION,
  collectDashboardSnapshot,
  discoverProjectCliSessions,
  generateAsdSessionId,
  handlePlan,
  isValidAsdSessionId,
  openProjectDb,
  promoteCliSessionToAsd,
  readHarnessTranscript,
  runDriverTurn,
  runSubagentControlAction,
  scanSubagentLogs,
  withReadOnlyProjectDb,
  type HarnessId,
  type SessionRow,
} from "./sdk.ts";
import {
  createTurnEventNormalizer,
  normalizeTranscriptMessages,
} from "./normalizer.ts";
import type {
  BoardTaskItem,
  DiscoveredSessionItem,
  GetBoardResponse,
  GetSessionsResponse,
  GetSubagentsResponse,
  GetTranscriptResponse,
  OrchestratorEvent,
  PromoteSessionRequest,
  PromoteSessionResponse,
  SubagentActionRequest,
  SubagentActionResponse,
  SubagentRecordItem,
  TurnStreamRequest,
} from "../shared/events.ts";

const MAX_JSON_BODY_BYTES = 2 * 1024 * 1024;

const VALID_HARNESSES = new Set<HarnessId>([
  "claude-code",
  "opencode",
  "codex",
  "antigravity",
  "cursor",
]);

const CONTENT_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

export interface OrchestratorDeps {
  discoverSessions?: typeof discoverProjectCliSessions;
  promoteSession?: typeof promoteCliSessionToAsd;
  readTranscript?: typeof readHarnessTranscript;
  collectSnapshot?: typeof collectDashboardSnapshot;
  scanSubagents?: typeof scanSubagentLogs;
  controlSubagent?: typeof runSubagentControlAction;
  runTurn?: typeof runDriverTurn;
  planSession?: typeof handlePlan;
  openDb?: typeof openProjectDb;
  readOnlyDb?: typeof withReadOnlyProjectDb;
}

export interface OrchestratorAppOptions {
  defaultProjectRoot?: string;
  homeDir?: string;
  staticWebDir?: string;
  deps?: OrchestratorDeps;
}

class HttpRequestError extends Error {
  statusCode: number;
  constructor(statusCode: number, message: string) {
    super(message);
    this.statusCode = statusCode;
  }
}

function parseHarness(raw: string | undefined | null, fallback?: HarnessId): HarnessId {
  const val = (raw ?? "").trim() as HarnessId;
  if (VALID_HARNESSES.has(val)) return val;
  if (fallback && !raw?.trim()) return fallback;
  throw new HttpRequestError(400, `Unsupported harness: ${raw ?? ""}`);
}

function resolveProjectRoot(raw: string | undefined | null, fallback: string): string {
  const trimmed = raw?.trim();
  return path.resolve(trimmed || fallback);
}

function sendJson(res: http.ServerResponse, statusCode: number, payload: unknown): void {
  if (res.writableEnded) return;
  const body = JSON.stringify(payload);
  res.writeHead(statusCode, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  res.end(body);
}

function writeSseEvent(res: http.ServerResponse, event: OrchestratorEvent): void {
  if (res.writableEnded || res.destroyed) return;
  res.write(`data: ${JSON.stringify(event)}\n\n`);
}

async function readJsonBody<T>(req: http.IncomingMessage): Promise<T> {
  const chunks: Buffer[] = [];
  let totalBytes = 0;

  for await (const chunk of req) {
    const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    totalBytes += buf.length;
    if (totalBytes > MAX_JSON_BODY_BYTES) {
      throw new HttpRequestError(413, "Request body too large");
    }
    chunks.push(buf);
  }

  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) return {} as T;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new HttpRequestError(400, "JSON body must be an object");
    }
    return parsed as T;
  } catch (err) {
    if (err instanceof HttpRequestError) throw err;
    throw new HttpRequestError(400, "Invalid JSON request body");
  }
}

function tryServeStaticAsset(
  reqPath: string,
  staticWebDir: string,
  res: http.ServerResponse
): boolean {
  if (!fs.existsSync(staticWebDir)) return false;
  const resolvedRoot = path.resolve(staticWebDir);
  const cleanPath = reqPath === "/" ? "/index.html" : reqPath;
  const target = path.resolve(resolvedRoot, `.${cleanPath}`);
  const isInside =
    target === resolvedRoot || target.startsWith(resolvedRoot + path.sep);

  let fileToServe = isInside && fs.existsSync(target) && fs.statSync(target).isFile()
    ? target
    : null;

  if (!fileToServe && !reqPath.startsWith("/api/")) {
    const indexPath = path.join(resolvedRoot, "index.html");
    if (fs.existsSync(indexPath) && fs.statSync(indexPath).isFile()) {
      fileToServe = indexPath;
    }
  }

  if (!fileToServe) return false;
  const ext = path.extname(fileToServe).toLowerCase();
  const contentType = CONTENT_TYPES[ext] ?? "application/octet-stream";
  const body = fs.readFileSync(fileToServe);
  res.writeHead(200, { "Content-Type": contentType });
  res.end(body);
  return true;
}

export function createOrchestratorHandler(
  options: OrchestratorAppOptions = {}
): http.RequestListener {
  const defaultProjectRoot = path.resolve(
    options.defaultProjectRoot ?? process.cwd()
  );
  const homeDir = options.homeDir;
  const staticWebDir = path.resolve(
    options.staticWebDir ?? path.join(process.cwd(), "dist", "web")
  );
  const deps: Required<OrchestratorDeps> = {
    discoverSessions: options.deps?.discoverSessions ?? discoverProjectCliSessions,
    promoteSession: options.deps?.promoteSession ?? promoteCliSessionToAsd,
    readTranscript: options.deps?.readTranscript ?? readHarnessTranscript,
    collectSnapshot: options.deps?.collectSnapshot ?? collectDashboardSnapshot,
    scanSubagents: options.deps?.scanSubagents ?? scanSubagentLogs,
    controlSubagent: options.deps?.controlSubagent ?? runSubagentControlAction,
    runTurn: options.deps?.runTurn ?? runDriverTurn,
    planSession: options.deps?.planSession ?? handlePlan,
    openDb: options.deps?.openDb ?? openProjectDb,
    readOnlyDb: options.deps?.readOnlyDb ?? withReadOnlyProjectDb,
  };

  async function handleRequest(
    req: http.IncomingMessage,
    res: http.ServerResponse
  ): Promise<void> {
    const method = (req.method ?? "GET").toUpperCase();
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    const pathname = url.pathname;

    // 1. Health
    if (method === "GET" && pathname === "/api/health") {
      sendJson(res, 200, {
        ok: true,
        version: ASD_VERSION,
        milestone: "M1",
        defaultProjectRoot,
      });
      return;
    }

    // 2. Sessions discovery
    if (method === "GET" && pathname === "/api/sessions") {
      const projectRoot = resolveProjectRoot(
        url.searchParams.get("projectRoot"),
        defaultProjectRoot
      );
      const limitParam = Number(url.searchParams.get("limit") ?? "100");
      const limit = Number.isFinite(limitParam) && limitParam > 0 ? limitParam : 100;
      const harnessParam = url.searchParams.get("harness");
      const harnesses = harnessParam
        ? [parseHarness(harnessParam)]
        : undefined;

      const discovered = deps.discoverSessions(projectRoot, {
        homeDir,
        limit,
        ...(harnesses ? { harnesses } : {}),
      });

      let dbSessions: SessionRow[] = [];
      let latestSessionId: string | null = null;
      try {
        const dbData = deps.readOnlyDb(
          projectRoot,
          (db) => ({
            sessions: db.listSessions(),
            latest: db.getLatestSession(),
          }),
          homeDir
        );
        dbSessions = dbData.sessions;
        latestSessionId = dbData.latest?.session_id ?? null;
      } catch {
        dbSessions = [];
      }

      const dbBySessionId = new Map<string, SessionRow>();
      for (const s of dbSessions) {
        dbBySessionId.set(s.session_id, s);
      }

      const seenAsdIds = new Set<string>();
      const items: DiscoveredSessionItem[] = discovered.map((entry) => {
        const linked = entry.asdSessionId
          ? dbBySessionId.get(entry.asdSessionId)
          : undefined;
        if (entry.asdSessionId) seenAsdIds.add(entry.asdSessionId);
        return {
          harness: entry.harness,
          harnessSessionId: entry.harnessSessionId,
          projectRoot: entry.projectRoot,
          updatedAt: entry.updatedAt,
          ...(entry.model ? { model: entry.model } : {}),
          ...(entry.summary ? { summary: entry.summary } : {}),
          ...(entry.transcriptPath ? { transcriptPath: entry.transcriptPath } : {}),
          asdSessionId: entry.asdSessionId ?? null,
          asdPhase: linked?.phase ?? null,
          isPromotedToAsd: Boolean(entry.asdSessionId),
        };
      });

      // Include ASD sessions created before their first native harness_session_id exists
      for (const s of dbSessions) {
        if (seenAsdIds.has(s.session_id)) continue;
        if (harnesses && !harnesses.includes(s.main_harness as HarnessId)) continue;
        items.push({
          harness: s.main_harness,
          harnessSessionId: s.harness_session_id ?? "",
          projectRoot: s.project_root,
          updatedAt: s.updated_at || s.created_at,
          ...(s.main_model ? { model: s.main_model } : {}),
          ...(s.memo ? { summary: s.memo } : {}),
          asdSessionId: s.session_id,
          asdPhase: s.phase,
          isPromotedToAsd: true,
        });
      }

      items.sort((a, b) => b.updatedAt - a.updatedAt);

      const payload: GetSessionsResponse = {
        sessions: items.slice(0, limit),
        activeSessionId: latestSessionId,
      };
      sendJson(res, 200, payload);
      return;
    }

    // 3. Promote CLI session -> ASD session
    if (method === "POST" && pathname === "/api/sessions/promote") {
      const body = await readJsonBody<PromoteSessionRequest>(req);
      const projectRoot = resolveProjectRoot(body.projectRoot, defaultProjectRoot);
      const harness = parseHarness(body.harness, "claude-code");
      const harnessSessionId = (body.harnessSessionId ?? "").trim();
      if (!harnessSessionId) {
        throw new HttpRequestError(400, "harnessSessionId is required");
      }

      const promoted = deps.promoteSession({
        projectRoot,
        homeDir,
        harness,
        harnessSessionId,
        model: body.model ?? null,
        title: body.title,
        memo: body.memo,
        transcriptPath: body.transcriptPath,
        makeForeground: Boolean(body.makeForeground),
        source: "gui",
      });

      const response: PromoteSessionResponse = {
        success: true,
        sessionId: promoted.sessionId,
        created: promoted.created,
      };
      sendJson(res, 200, response);
      return;
    }

    // 4. Transcript restoration
    if (method === "GET" && pathname.startsWith("/api/transcript/")) {
      const rest = pathname.slice("/api/transcript/".length);
      const slashIdx = rest.indexOf("/");
      if (slashIdx <= 0 || slashIdx === rest.length - 1) {
        throw new HttpRequestError(
          400,
          "Expected /api/transcript/:harness/:id"
        );
      }
      const harness = parseHarness(decodeURIComponent(rest.slice(0, slashIdx)));
      const harnessSessionId = decodeURIComponent(rest.slice(slashIdx + 1)).trim();
      const transcriptPath = url.searchParams.get("transcriptPath")?.trim() || undefined;
      const maxMessagesRaw = Number(
        url.searchParams.get("maxMessages") ?? url.searchParams.get("limit") ?? "300"
      );
      const limit =
        Number.isFinite(maxMessagesRaw) && maxMessagesRaw > 0
          ? maxMessagesRaw
          : 300;

      const transcript = deps.readTranscript(harness, harnessSessionId, {
        homeDir,
        ...(transcriptPath ? { transcriptPath } : {}),
        limit,
      });

      const messages = normalizeTranscriptMessages(transcript.messages);
      const response: GetTranscriptResponse = {
        harness,
        harnessSessionId,
        messages,
        fullCount: messages.length,
      };
      sendJson(res, 200, response);
      return;
    }

    // 5. Board snapshot
    if (method === "GET" && pathname === "/api/board") {
      const projectRoot = resolveProjectRoot(
        url.searchParams.get("projectRoot"),
        defaultProjectRoot
      );
      const requestedSessionId = url.searchParams.get("sessionId")?.trim();
      const snapshot = await deps.collectSnapshot({
        projectRoot,
        homeDir,
        skipCliSpawn: true,
        syncLocalTranscripts: false,
      });

      const targetSession =
        (requestedSessionId
          ? snapshot.sessions.find((s) => s.session_id === requestedSessionId)
          : snapshot.session) ?? snapshot.session;
      const targetTasks = requestedSessionId
        ? snapshot.allTasks.filter((t) => t.session_id === requestedSessionId)
        : snapshot.tasks;

      const doneCount = targetTasks.filter((t) => t.status === "done").length;
      const tasks: BoardTaskItem[] = targetTasks.map((t) => ({
        id: t.task_id,
        subject: t.title,
        status: t.status,
        assigneeRole: t.role,
        difficulty: t.difficulty,
        turn: t.turn,
      }));

      const response: GetBoardResponse = {
        ...(targetSession?.session_id ? { sessionId: targetSession.session_id } : {}),
        ...(targetSession?.phase ? { phase: targetSession.phase } : {}),
        progress: `${doneCount}/${tasks.length}`,
        tasks,
      };
      sendJson(res, 200, response);
      return;
    }

    // 6. Subagent log listing
    if (method === "GET" && pathname === "/api/subagents") {
      const projectRoot = resolveProjectRoot(
        url.searchParams.get("projectRoot"),
        defaultProjectRoot
      );
      const filterSessionId = url.searchParams.get("sessionId")?.trim();
      const filterTaskId = url.searchParams.get("taskId")?.trim();

      const records = deps.scanSubagents(projectRoot).filter((r) => {
        if (filterSessionId && r.sessionId !== filterSessionId) return false;
        if (filterTaskId && r.taskId !== filterTaskId) return false;
        return true;
      });

      const subagents: SubagentRecordItem[] = records.map((r) => ({
        subSessionId: r.subSessionId,
        parentSessionId: r.sessionId,
        taskId: r.taskId,
        role: r.role,
        harness: r.harness,
        model: r.model,
        status: r.status,
        startTime: new Date(r.startedAt).toISOString(),
        ...(r.finishedAt ? { endTime: new Date(r.finishedAt).toISOString() } : {}),
        logPath: path.join(r.dirPath, "output.log"),
        promptPath: path.join(r.dirPath, "prompt.md"),
      }));

      const response: GetSubagentsResponse = { subagents };
      sendJson(res, 200, response);
      return;
    }

    // 7. Subagent control action
    if (method === "POST" && pathname === "/api/subagent-action") {
      const body = await readJsonBody<SubagentActionRequest>(req);
      const projectRoot = resolveProjectRoot(body.projectRoot, defaultProjectRoot);
      const sessionId = (body.sessionId ?? "").trim();
      const subSessionId = (body.subSessionId ?? "").trim();
      const action = body.action;
      if (!sessionId || !subSessionId || !action) {
        throw new HttpRequestError(
          400,
          "sessionId, subSessionId, and action are required"
        );
      }
      const allowedActions = new Set(["kill", "retry", "fallback", "custom", "resume"]);
      if (!allowedActions.has(action)) {
        throw new HttpRequestError(400, `Unsupported subagent action: ${action}`);
      }

      const result = await deps.controlSubagent({
        projectRoot,
        homeDir,
        sessionId,
        subSessionId,
        action: action as "kill" | "retry" | "fallback" | "custom" | "resume",
        ...(body.harness ? { harness: parseHarness(body.harness) } : {}),
        ...(body.model ? { model: body.model } : {}),
        ...(body.reason ? { prompt: body.reason } : {}),
      });

      const response: SubagentActionResponse = {
        success: result.ok,
        message: `${result.action}:${result.status}`,
      };
      sendJson(res, 200, response);
      return;
    }

    // 8. Real-time SSE turn execution
    if (method === "POST" && pathname === "/api/turn/stream") {
      const body = await readJsonBody<TurnStreamRequest>(req);
      const prompt = (body.prompt ?? "").trim();
      if (!prompt) {
        throw new HttpRequestError(400, "prompt is required");
      }

      const projectRoot = resolveProjectRoot(body.projectRoot, defaultProjectRoot);
      const harness = parseHarness(body.harness, "claude-code");
      const requestedModel = body.model?.trim() || undefined;
      const requestedSessionId = body.sessionId?.trim();
      if (requestedSessionId && !isValidAsdSessionId(requestedSessionId)) {
        throw new HttpRequestError(400, "Invalid ASD sessionId");
      }

      let sessionId = requestedSessionId || generateAsdSessionId();
      let needsCreate = !requestedSessionId;
      let needsUpdate = false;

      if (requestedSessionId) {
        const inspect = deps.readOnlyDb(
          projectRoot,
          (db) => {
            const existing = db.getSession(requestedSessionId);
            const active = existing ? db.getActiveDriver(requestedSessionId) : undefined;
            return { existing, active };
          },
          homeDir
        );

        if (!inspect.existing) {
          needsCreate = true;
        } else if (
          !inspect.active ||
          inspect.active.harness !== harness ||
          (requestedModel && inspect.active.model !== requestedModel)
        ) {
          needsUpdate = true;
        }
      }

      if (needsCreate) {
        const created = await deps.planSession(
          {
            action: "create",
            sessionId,
            mainHarness: harness,
            ...(requestedModel ? { mainModel: requestedModel } : {}),
            autoApprove: true,
          },
          { projectRoot, homeDir }
        );
        sessionId = created.sessionId;
      } else if (needsUpdate) {
        await deps.planSession(
          {
            action: "update",
            sessionId,
            mainHarness: harness,
            ...(requestedModel ? { mainModel: requestedModel } : {}),
            autoApprove: true,
          },
          { projectRoot, homeDir }
        );
      }

      const turnId = `turn-${crypto.randomUUID().slice(0, 8)}`;
      const normalizer = createTurnEventNormalizer({ sessionId, turnId });
      const abortController = new AbortController();

      res.on("close", () => {
        if (!res.writableEnded) {
          abortController.abort();
        }
      });

      res.writeHead(200, {
        "Content-Type": "text/event-stream; charset=utf-8",
        "Cache-Control": "no-cache, no-transform",
        Connection: "keep-alive",
        "X-Accel-Buffering": "no",
      });
      res.flushHeaders?.();

      writeSseEvent(res, {
        kind: "status",
        sessionId,
        turnId,
        status: "running",
        timestamp: new Date().toISOString(),
      });

      let errorEmitted = false;
      const emit = (ev: OrchestratorEvent): void => {
        if (ev.kind === "error") errorEmitted = true;
        writeSseEvent(res, ev);
      };

      const db = deps.openDb(projectRoot, homeDir);
      try {
        const result = await deps.runTurn({
          projectRoot,
          homeDir,
          db,
          sessionId,
          prompt,
          streamJson: true,
          signal: abortController.signal,
          onOutput: (chunk: string) => {
            for (const ev of normalizer.onOutputChunk(chunk)) {
              emit(ev);
            }
          },
          onEvent: (driverEvent) => {
            for (const ev of normalizer.fromDriverEvent(driverEvent)) {
              emit(ev);
            }
          },
        });

        const harnessSessionId =
          result.harnessSessionId ?? normalizer.getHarnessSessionId();
        const succeeded = result.exitCode === 0 && !result.timedOut && !abortController.signal.aborted;

        if (!succeeded && !errorEmitted) {
          emit({
            kind: "error",
            sessionId,
            turnId,
            error: result.timedOut
              ? "Driver turn timed out"
              : `Driver exited with code ${result.exitCode}`,
            timestamp: new Date().toISOString(),
          });
        }

        emit({
          kind: "status",
          sessionId,
          turnId,
          status: succeeded ? "done" : "interrupted",
          timestamp: new Date().toISOString(),
          raw: {
            driverId: result.driverId,
            runId: result.runId,
            harnessSessionId,
            exitCode: result.exitCode,
            timedOut: result.timedOut,
            durationMs: result.durationMs,
          },
        });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        if (!errorEmitted) {
          emit({
            kind: "error",
            sessionId,
            turnId,
            error: msg,
            timestamp: new Date().toISOString(),
          });
        }
        emit({
          kind: "status",
          sessionId,
          turnId,
          status: "interrupted",
          timestamp: new Date().toISOString(),
        });
      } finally {
        try {
          db.close();
        } catch {
          // ignore close errors
        }
        if (!res.writableEnded) {
          res.end();
        }
      }
      return;
    }

    // 9. Static web bundle fallback (production)
    if (method === "GET" && tryServeStaticAsset(pathname, staticWebDir, res)) {
      return;
    }

    sendJson(res, 404, { error: "Not found", path: pathname });
  }

  return (req, res) => {
    void handleRequest(req, res).catch((err) => {
      if (err instanceof HttpRequestError) {
        sendJson(res, err.statusCode, { error: err.message });
        return;
      }
      const message = err instanceof Error ? err.message : String(err);
      if (!res.headersSent) {
        sendJson(res, 500, { error: message });
      } else if (!res.writableEnded) {
        writeSseEvent(res, {
          kind: "error",
          error: message,
          timestamp: new Date().toISOString(),
        });
        res.end();
      }
    });
  };
}

export function createOrchestratorServer(
  options: OrchestratorAppOptions = {}
): http.Server {
  return http.createServer(createOrchestratorHandler(options));
}
