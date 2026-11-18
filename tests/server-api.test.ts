import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as http from "node:http";
import type { AddressInfo } from "node:net";
import * as child_process from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as sdk from "../src/server/sdk.ts";
import { createOrchestratorApp } from "../src/server/app.ts";
import type { OrchestratorDeps, AppOptions } from "../src/server/app.ts";
import {
  SIDECAR_READY_PROTOCOL,
  type SidecarReadyInfo,
  type GetHealthResponse,
  type ListProjectsResponse,
  type SelectProjectResponse,
  type GetModelsResponse,
  type GetRoutingResponse,
  type SaveRoutingResponse,
  type GetSessionsResponse,
  type CreateSessionResponse,
  type PromoteSessionResponse,
  type DemoteSessionResponse,
  type UnlinkSessionResponse,
  type DeleteSessionResponse,
  type SaveSessionMemoResponse,
  type SetForegroundSessionResponse,
  type SetSessionPhaseResponse,
  type GetResumeCommandResponse,
  type GetTranscriptResponse,
  type TurnCancelResponse,
  type GetBoardResponse,
  type GetPlanResponse,
  type SavePlanResponse,
  type PlanSyncResponse,
  type DispatchResponse,
  type DebateResponse,
  type WorktreeDiffResponse,
  type ApproveTaskResponse,
  type RejectTaskResponse,
  type GetSubagentsResponse,
  type SubagentLogTailResponse,
  type SubagentActionResponse,
  type TerminalOpenResponse,
  type GetHarnessCommandsResponse,
  type FsListResponse,
  type OrchestratorEvent,
} from "../src/shared/protocol.ts";
import { resolveSlashCommandPromptForTurn } from "../src/server/routes/commands.ts";

async function startTestServer(
  deps?: Partial<OrchestratorDeps>,
  options?: AppOptions
) {
  const server = createOrchestratorApp(deps, options);
  await new Promise<void>((resolve) =>
    server.listen(0, "127.0.0.1", () => resolve())
  );
  const port = (server.address() as AddressInfo).port;
  const baseUrl = `http://127.0.0.1:${port}`;
  const close = () =>
    new Promise<void>((resolve) => server.close(() => resolve()));
  return { server, port, baseUrl, close };
}

describe("Clean-room Sidecar HTTP/SSE Server & Ported APIs", () => {
  describe("Authentication", () => {
    it("returns 401 when token is configured and request lacks valid bearer token", async () => {
      const { baseUrl, close } = await startTestServer(
        { version: "2.6.0" },
        { token: "secret-token-123" }
      );
      try {
        // No header
        const res1 = await fetch(`${baseUrl}/api/health`);
        assert.equal(res1.status, 401);
        const data1 = await res1.json();
        assert.ok((data1 as any).error.includes("Unauthorized"));

        // Wrong token
        const res2 = await fetch(`${baseUrl}/api/health`, {
          headers: { Authorization: "Bearer wrong-token" },
        });
        assert.equal(res2.status, 401);

        // Valid token
        const res3 = await fetch(`${baseUrl}/api/health`, {
          headers: { Authorization: "Bearer secret-token-123" },
        });
        assert.equal(res3.status, 200);
        const data3 = (await res3.json()) as GetHealthResponse;
        assert.equal(data3.ok, true);
      } finally {
        await close();
      }
    });

    it("allows requests when no token is configured", async () => {
      const { baseUrl, close } = await startTestServer({ version: "2.6.0" });
      try {
        const res = await fetch(`${baseUrl}/api/health`);
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetHealthResponse;
        assert.equal(data.ok, true);
        assert.equal(data.version, "2.6.0");
      } finally {
        await close();
      }
    });
  });

  describe("Projects API", () => {
    it("GET /api/projects lists known projects", async () => {
      const mockProjects = [
        {
          projectRoot: "/fake/root/a",
          projectHash: "hash-a",
          sessionCount: 3,
          updatedAt: 1234567,
          dbPath: "/fake/db",
          existsOnDisk: true,
        },
      ];
      const { baseUrl, close } = await startTestServer({
        listKnownProjects: () => mockProjects as any,
      });
      try {
        const res = await fetch(`${baseUrl}/api/projects?skipDiskCheck=true`);
        assert.equal(res.status, 200);
        const data = (await res.json()) as ListProjectsResponse;
        assert.equal(data.projects.length, 1);
        assert.equal(data.projects[0]?.projectRoot, "/fake/root/a");
        assert.equal(data.projects[0]?.projectHash, "hash-a");
        assert.equal(data.projects[0]?.lastActiveAt, 1234567);
      } finally {
        await close();
      }
    });

    it("POST /api/projects/select initializes and returns resolved paths", async () => {
      let initialized = false;
      const { baseUrl, close } = await startTestServer({
        ensureProjectInitialized: () => {
          initialized = true;
        },
        resolveAsdPaths: (root: string) => ({
          projectHash: "hash-sel",
          globalStateDir: "/global/.asd",
          dbPath: "/global/.asd/state/core.db",
          lastSyncPath: "/global/.asd/state/last-sync.json",
          globalRoutingPath: "/global/.asd/routing.jsonc",
          globalMcpPath: "/global/.asd/mcp.jsonc",
          localViewDir: path.join(root, ".asd"),
          localRoutingPath: path.join(root, ".asd/routing.jsonc"),
          planMdPath: path.join(root, ".asd/PLAN.md"),
          sessionsDir: path.join(root, ".asd/sessions"),
          worktreesDir: path.join(root, ".asd/wt"),
        }),
      });
      try {
        const res = await fetch(`${baseUrl}/api/projects/select`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ projectRoot: "/my/project", initialize: true }),
        });
        assert.equal(res.status, 200);
        const data = (await res.json()) as SelectProjectResponse;
        assert.equal(data.success, true);
        assert.equal(data.projectHash, "hash-sel");
        assert.equal(data.projectRoot, "/my/project");
        assert.ok(data.paths.planMdPath);
        assert.equal(initialized, true);
      } finally {
        await close();
      }
    });
  });

  describe("Models & Routing API", () => {
    it("GET /api/models and POST /api/models/refresh return catalog", async () => {
      let refreshed = false;
      const { baseUrl, close } = await startTestServer({
        discoverHarnessModels: async (opts) => {
          if (opts?.forceRefresh) refreshed = true;
          return { "claude-code": ["claude-3-7-sonnet"] } as any;
        },
      });
      try {
        const res1 = await fetch(`${baseUrl}/api/models`);
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as GetModelsResponse;
        assert.deepEqual(data1.models["claude-code"], ["claude-3-7-sonnet"]);

        const res2 = await fetch(`${baseUrl}/api/models/refresh`, {
          method: "POST",
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as GetModelsResponse;
        assert.deepEqual(data2.models["claude-code"], ["claude-3-7-sonnet"]);
        assert.equal(refreshed, true);
      } finally {
        await close();
      }
    });

    it("GET /api/routing and POST /api/routing load and save routing config", async () => {
      let savedConfig: any = null;
      const { baseUrl, close } = await startTestServer({
        loadRoutingConfig: () => ({ default: "claude-3-7-sonnet" } as any),
        saveProjectRoutingConfig: (opts) => {
          savedConfig = opts;
        },
      });
      try {
        const res1 = await fetch(`${baseUrl}/api/routing?projectRoot=/p`);
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as GetRoutingResponse;
        assert.equal((data1.routing as any).default, "claude-3-7-sonnet");

        const res2 = await fetch(`${baseUrl}/api/routing`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            action: "save",
            routingConfig: { default: "claude-3-5-haiku" },
          }),
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as SaveRoutingResponse;
        assert.equal(data2.success, true);
        assert.deepEqual(savedConfig.routingConfig, {
          default: "claude-3-5-haiku",
        });
      } finally {
        await close();
      }
    });
  });

  describe("Sessions API", () => {
    it("GET /api/sessions lists sessions and foreground session", async () => {
      const { baseUrl, close } = await startTestServer({
        discoverProjectCliSessions: () => [
          {
            harness: "claude-code" as any,
            harnessSessionId: "sid-001",
            projectRoot: "/p",
            updatedAt: 1000,
            model: "claude-3-7",
            summary: "first session",
            transcriptPath: "/p/t.jsonl",
            asdSessionId: "asd-001",
            startedAt: 900,
          },
        ],
        getForegroundSessionId: () => "asd-001",
      });
      try {
        const res = await fetch(`${baseUrl}/api/sessions?projectRoot=/p`);
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetSessionsResponse;
        assert.equal(data.sessions.length, 1);
        assert.equal(data.sessions[0]?.harnessSessionId, "sid-001");
        assert.equal(data.sessions[0]?.isPromotedToAsd, true);
        assert.equal(data.activeSessionId, "asd-001");
      } finally {
        await close();
      }
    });

    it("POST /api/sessions creates a new session", async () => {
      let createdSession: any = null;
      let foregroundSet = false;
      const { baseUrl, close } = await startTestServer({
        generateAsdSessionId: () => "asd-new-999",
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = {
            createOrGetSession: (s: any) => {
              createdSession = s;
            },
          };
          return fn(fakeDb as any);
        },
        setForegroundSession: () => {
          foregroundSet = true;
        },
      });
      try {
        const res = await fetch(`${baseUrl}/api/sessions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            harness: "opencode",
            model: "glm-5",
            memo: "Test session",
            makeForeground: true,
          }),
        });
        assert.equal(res.status, 200);
        const data = (await res.json()) as CreateSessionResponse;
        assert.equal(data.success, true);
        assert.equal(data.sessionId, "asd-new-999");
        assert.equal(data.phase, "planning");
        assert.equal(createdSession.sessionId, "asd-new-999");
        assert.equal(createdSession.mainHarness, "opencode");
        assert.equal(foregroundSet, true);
      } finally {
        await close();
      }
    });

    it("POST /api/sessions/promote promotes CLI session", async () => {
      const { baseUrl, close } = await startTestServer({
        promoteCliSessionToAsd: () =>
          ({
            sessionId: "asd-promoted-123",
            created: true,
          } as any),
      });
      try {
        const res = await fetch(`${baseUrl}/api/sessions/promote`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            harness: "claude-code",
            harnessSessionId: "raw-uuid",
          }),
        });
        assert.equal(res.status, 200);
        const data = (await res.json()) as PromoteSessionResponse;
        assert.equal(data.success, true);
        assert.equal(data.sessionId, "asd-promoted-123");
        assert.equal(data.created, true);
      } finally {
        await close();
      }
    });

    it("POST /api/sessions/demote demotes session and returns cliString", async () => {
      const { baseUrl, close } = await startTestServer({
        buildSessionDemotion: () =>
          ({
            cliString: 'claude --resume "sess-raw"',
            harness: "claude-code",
            harnessSessionId: "sess-raw",
          } as any),
      });
      try {
        const res = await fetch(`${baseUrl}/api/sessions/demote`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ projectRoot: "/p", sessionId: "asd-demote" }),
        });
        assert.equal(res.status, 200);
        const data = (await res.json()) as DemoteSessionResponse;
        assert.equal(data.success, true);
        assert.equal(data.cliString, 'claude --resume "sess-raw"');
      } finally {
        await close();
      }
    });

    it("POST /api/sessions/unlink and DELETE /api/sessions/:id unlink/delete session", async () => {
      let unlinked = false;
      let deleted = false;
      const { baseUrl, close } = await startTestServer({
        unlinkHarnessSessionFromAsdSession: (opts) => {
          if (opts.deleteAsdSession) deleted = true;
          else unlinked = true;
        },
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = { deleteSession: () => {} };
          return fn(fakeDb as any);
        },
      });
      try {
        const res1 = await fetch(`${baseUrl}/api/sessions/unlink`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            asdSessionId: "asd-1",
            deleteAsdSession: false,
          }),
        });
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as UnlinkSessionResponse;
        assert.equal(data1.success, true);
        assert.equal(unlinked, true);

        const res2 = await fetch(`${baseUrl}/api/sessions/asd-1?projectRoot=/p`, {
          method: "DELETE",
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as DeleteSessionResponse;
        assert.equal(data2.success, true);
        assert.equal(deleted, true);
      } finally {
        await close();
      }
    });

    it("POST /api/sessions/memo, /foreground, /phase update session state", async () => {
      let memoSaved = "";
      let phaseUpdated = "";
      let foregroundCalled = false;
      const { baseUrl, close } = await startTestServer({
        saveSessionMemo: (opts) => {
          memoSaved = opts.memo;
        },
        getForegroundSessionId: () => "prev-sess",
        setForegroundSession: () => {
          foregroundCalled = true;
        },
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = {
            updateSessionPhase: (_sid: string, ph: string) => {
              phaseUpdated = ph;
            },
          };
          return fn(fakeDb as any);
        },
      });
      try {
        const res1 = await fetch(`${baseUrl}/api/sessions/memo`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-1",
            memo: "New note",
          }),
        });
        assert.equal(res1.status, 200);
        assert.equal(memoSaved, "New note");

        const res2 = await fetch(`${baseUrl}/api/sessions/foreground`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ projectRoot: "/p", sessionId: "s-1" }),
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as SetForegroundSessionResponse;
        assert.equal(data2.previousSessionId, "prev-sess");
        assert.equal(foregroundCalled, true);

        const res3 = await fetch(`${baseUrl}/api/sessions/phase`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-1",
            phase: "running",
          }),
        });
        assert.equal(res3.status, 200);
        assert.equal(phaseUpdated, "running");
      } finally {
        await close();
      }
    });

    it("GET /api/sessions/:id/resume-command builds interactive command", async () => {
      const { baseUrl, close } = await startTestServer({
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = {
            getSession: () => ({
              main_harness: "opencode",
              harness_session_id: "ses_abc",
              main_model: "glm-5",
            }),
            getActiveDriver: () => null,
          };
          return fn(fakeDb as any);
        },
        buildInteractiveResumeHarnessCommand: (_harness, sid, _model) => ({
          command: "opencode",
          args: ["run", "--attach", sid || ""],
          cliString: `opencode run --attach ${sid || ""}`,
        }),
      });
      try {
        const res = await fetch(
          `${baseUrl}/api/sessions/asd-123/resume-command?projectRoot=/p`
        );
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetResumeCommandResponse;
        assert.equal(data.success, true);
        assert.equal(data.command, "opencode");
        assert.deepEqual(data.args, ["run", "--attach", "ses_abc"]);
        assert.equal(data.cliString, "opencode run --attach ses_abc");
      } finally {
        await close();
      }
    });
  });

  describe("Transcripts API", () => {
    it("GET /api/transcript/:harness/:id normalizes transcript messages", async () => {
      const { baseUrl, close } = await startTestServer({
        readHarnessTranscript: () => ({
          found: true,
          harness: "claude-code" as any,
          sessionId: "sess-01",
          messages: [
            { index: 0, role: "user" as const, text: "hello" },
            {
              index: 1,
              role: "assistant" as const,
              text: "world",
              category: "assistant" as const,
            },
          ],
          hasMore: false,
          truncated: false,
        }),
      });
      try {
        const res = await fetch(
          `${baseUrl}/api/transcript/claude-code/sess-01?projectRoot=/p`
        );
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetTranscriptResponse;
        assert.equal(data.harness, "claude-code");
        assert.equal(data.harnessSessionId, "sess-01");
        assert.equal(data.messages.length, 2);
        assert.equal(data.messages[0]?.category, "user");
        assert.equal(data.messages[0]?.text, "hello");
        assert.equal(data.messages[1]?.category, "assistant");
        assert.equal(data.messages[1]?.text, "world");
      } finally {
        await close();
      }
    });

    it("GET /api/transcript/opencode/:id prefers opencode.db over mirrored hook-bridge jsonl stub", async () => {
      const seenPaths: Array<string | undefined> = [];
      const { baseUrl, close } = await startTestServer({
        readHarnessTranscript: (_harness, _sid, opts) => {
          seenPaths.push(opts?.transcriptPath);
          return {
            found: true,
            harness: "opencode" as any,
            sessionId: "ses_abc",
            transcriptPath: opts?.transcriptPath || "/home/.local/share/opencode/opencode.db",
            messages: [
              { index: 0, role: "user" as const, text: "hi", category: "user" as const },
              { index: 1, role: "assistant" as const, text: "hello", category: "assistant" as const },
            ],
            hasMore: false,
            truncated: false,
          };
        },
      });
      try {
        const res = await fetch(
          `${baseUrl}/api/transcript/opencode/ses_abc?projectRoot=/p&transcriptPath=${encodeURIComponent("C:/Users/me/.claude/transcripts/ses_abc.jsonl")}`
        );
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetTranscriptResponse;
        assert.equal(data.messages.length, 2);
        assert.deepEqual(seenPaths, [undefined]);
      } finally {
        await close();
      }
    });
  });

  describe("Turn Streaming & Cancellation", () => {
    function createMockTurnDbDeps() {
      let closed = false;
      const sessions = new Map<string, any>();
      const drivers = new Map<string, any>();
      return {
        isClosed: () => closed,
        deps: {
          openProjectDb: () =>
            ({
              getSession: (id: string) => sessions.get(id),
              createOrGetSession: (s: any) => {
                const row = { session_id: s.sessionId, ...s };
                sessions.set(s.sessionId, row);
                return row;
              },
              findSessionByHarnessSessionId: () => undefined,
              getActiveDriver: (id: string) => drivers.get(id),
              close: () => {
                closed = true;
              },
            } as any),
          attachDriver: (_db: any, input: any) => {
            const driver = {
              driver_id: "drv-mock-1",
              session_id: input.sessionId,
              harness: input.harness,
              model: input.model ?? null,
              harness_session_id: input.harnessSessionId ?? null,
              source: input.source ?? "user",
              attached_at: Date.now(),
              detached_at: null,
              detach_reason: null,
              briefing_injected: 0,
            };
            drivers.set(input.sessionId, driver);
            return { driver, previousDriverId: null, changed: true };
          },
        },
      };
    }

    it("POST /api/turn/stream emits SSE OrchestratorEvent stream and completes", async () => {
      const mockDb = createMockTurnDbDeps();
      const { baseUrl, close } = await startTestServer({
        ...mockDb.deps,
        runDriverTurn: async (input) => {
          assert.ok(input.db);
          input.onEvent({
            type: "thinking",
            harness: "claude-code",
            thinking: "Analyzing problem",
            timestamp: Date.now(),
          });
          input.onEvent({
            type: "text",
            harness: "claude-code",
            text: "Hello user",
            timestamp: Date.now(),
          });
          return { exitCode: 0, stdout: "", stderr: "" } as any;
        },
      });
      try {
        const res = await fetch(`${baseUrl}/api/turn/stream`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Accept: "text/event-stream",
          },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "asd-turn-1",
            harness: "claude-code",
            prompt: "Say hi",
          }),
        });

        assert.equal(res.status, 200);
        assert.ok(
          res.headers.get("content-type")?.includes("text/event-stream")
        );

        const reader = res.body?.getReader();
        assert.ok(reader);

        const decoder = new TextDecoder();
        let buffer = "";
        const events: OrchestratorEvent[] = [];

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const parts = buffer.split("\n\n");
          buffer = parts.pop() || "";
          for (const part of parts) {
            const line = part.trim();
            if (line.startsWith("data: ")) {
              events.push(JSON.parse(line.slice(6)));
            }
          }
        }

        assert.ok(events.length >= 3);
        assert.equal(events[0]?.kind, "status");
        assert.equal(events[0]?.status, "running");

        const thinkingEvt = events.find((e) => e.kind === "thinking");
        assert.ok(thinkingEvt);
        assert.equal(thinkingEvt.thinking, "Analyzing problem");

        const textEvt = events.find((e) => e.kind === "text");
        assert.ok(textEvt);
        assert.equal(textEvt.text, "Hello user");

        const lastEvt = events.at(-1);
        assert.equal(lastEvt?.kind, "status");
        assert.equal(lastEvt?.status, "done");
        assert.equal(mockDb.isClosed(), true);
      } finally {
        await close();
      }
    });

    it("POST /api/turn/cancel cancels turn and emits interrupted status", async () => {
      let turnIdCaptured = "";
      const mockDb = createMockTurnDbDeps();
      const { baseUrl, close } = await startTestServer({
        ...mockDb.deps,
        runDriverTurn: async (input) => {
          return new Promise<any>((resolve, reject) => {
            input.signal.addEventListener("abort", () => {
              const err = new Error("AbortError");
              err.name = "AbortError";
              reject(err);
            });
          });
        },
      });
      try {
        const streamPromise = (async () => {
          const res = await fetch(`${baseUrl}/api/turn/stream`, {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Accept: "text/event-stream",
            },
            body: JSON.stringify({
              projectRoot: "/p",
              sessionId: "asd-turn-cancel",
              harness: "claude-code",
              prompt: "Long task",
            }),
          });
          const reader = res.body?.getReader();
          assert.ok(reader);
          const decoder = new TextDecoder();
          let buf = "";
          const evts: OrchestratorEvent[] = [];
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buf += decoder.decode(value, { stream: true });
            const parts = buf.split("\n\n");
            buf = parts.pop() || "";
            for (const p of parts) {
              const line = p.trim();
              if (line.startsWith("data: ")) {
                const evt = JSON.parse(line.slice(6)) as OrchestratorEvent;
                evts.push(evt);
                if (evt.turnId && !turnIdCaptured) {
                  turnIdCaptured = evt.turnId;
                }
              }
            }
          }
          return evts;
        })();

        // Wait until turnId is emitted
        while (!turnIdCaptured) {
          await new Promise((r) => setTimeout(r, 10));
        }

        // Cancel turn
        const cancelRes = await fetch(`${baseUrl}/api/turn/cancel`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ turnId: turnIdCaptured }),
        });
        assert.equal(cancelRes.status, 200);
        const cancelData = (await cancelRes.json()) as TurnCancelResponse;
        assert.equal(cancelData.success, true);

        const streamEvents = await streamPromise;
        const lastEvt = streamEvents.at(-1);
        assert.equal(lastEvt?.kind, "status");
        assert.equal(lastEvt?.status, "interrupted");
        assert.equal(mockDb.isClosed(), true);
      } finally {
        await close();
      }
    });

    it("POST /api/turn/stream attaches driver in real SQLite, preserves harnessSessionId on Turn 2, and emits usage from raw output", async () => {
      const tmpProject = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-turn-db-"));
      const tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-turn-home-"));
      let callCount = 0;
      const seenActiveDriverSids: Array<string | null> = [];

      const { baseUrl, close } = await startTestServer(
        {
          runDriverTurn: async (input) => {
            callCount++;
            assert.ok(input.db);
            assert.ok(sdk.isValidAsdSessionId(input.sessionId));
            const active = input.db.getActiveDriver(input.sessionId);
            assert.ok(active);
            assert.equal(active.harness, "codex");
            assert.equal(active.model, "gpt-6-luna");
            seenActiveDriverSids.push(active.harness_session_id ?? null);

            if (callCount === 1) {
              input.onOutput?.(
                JSON.stringify({ type: "thread.started", thread_id: "thr-real-001" }) + "\n"
              );
              input.onOutput?.(
                JSON.stringify({
                  type: "item.completed",
                  item: { id: "m1", type: "agent_message", text: "Turn 1 OK" },
                }) + "\n"
              );
              input.onOutput?.(
                JSON.stringify({
                  type: "turn.completed",
                  usage: { input_tokens: 12, output_tokens: 4 },
                }) + "\n"
              );
              sdk.attachDriver(input.db, {
                projectRoot: input.projectRoot,
                sessionId: input.sessionId,
                harness: "codex",
                model: "gpt-6-luna",
                harnessSessionId: "thr-real-001",
                source: "user",
              });
              return { exitCode: 0, harnessSessionId: "thr-real-001" } as any;
            }

            input.onOutput?.(
              JSON.stringify({
                type: "item.completed",
                item: { id: "m2", type: "agent_message", text: "Turn 2 Resumed" },
              }) + "\n"
            );
            return { exitCode: 0, harnessSessionId: active.harness_session_id ?? null } as any;
          },
        },
        { defaultProjectRoot: tmpProject, homeDir: tmpHome }
      );

      async function collectTurnEvents(payload: Record<string, unknown>) {
        const res = await fetch(`${baseUrl}/api/turn/stream`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Accept: "text/event-stream",
          },
          body: JSON.stringify(payload),
        });
        assert.equal(res.status, 200);
        const text = await res.text();
        return text
          .split("\n\n")
          .map((chunk) => chunk.trim())
          .filter((line) => line.startsWith("data: "))
          .map((line) => JSON.parse(line.slice(6)) as OrchestratorEvent);
      }

      try {
        // Turn 1 with omitted sessionId -> generates ASD session + attaches driver
        const t1Events = await collectTurnEvents({
          projectRoot: tmpProject,
          harness: "codex",
          model: "gpt-6-luna",
          prompt: "First turn",
        });
        const asdSessionId = t1Events[0]?.sessionId;
        assert.ok(asdSessionId && asdSessionId.startsWith("asd-"));
        assert.ok(t1Events.some((e) => e.kind === "text" && e.text === "Turn 1 OK"));
        const usageEvt = t1Events.find((e) => e.kind === "usage");
        assert.equal(usageEvt?.usage?.inputTokens, 12);
        assert.equal(usageEvt?.usage?.outputTokens, 4);
        const t1Done = t1Events.at(-1);
        assert.equal(t1Done?.status, "done");
        assert.equal((t1Done?.raw as any)?.harnessSessionId, "thr-real-001");

        // Turn 2 reusing asdSessionId -> preserves driver.harness_session_id ("thr-real-001")
        const t2Events = await collectTurnEvents({
          projectRoot: tmpProject,
          sessionId: asdSessionId,
          harness: "codex",
          model: "gpt-6-luna",
          prompt: "Second turn",
        });
        assert.ok(t2Events.some((e) => e.kind === "text" && e.text === "Turn 2 Resumed"));
        assert.equal(t2Events.at(-1)?.status, "done");
        assert.deepEqual(seenActiveDriverSids, [null, "thr-real-001"]);
      } finally {
        await close();
        fs.rmSync(tmpProject, { recursive: true, force: true });
        fs.rmSync(tmpHome, { recursive: true, force: true });
      }
    });
  });

  describe("Board, Plan, Dispatch & Debate", () => {
    it("GET /api/board maps snapshot tasks to BoardTaskItem[]", async () => {
      const { baseUrl, close } = await startTestServer({
        collectDashboardSnapshot: async () => ({
          session: { session_id: "s-board", phase: "running" } as any,
          tasks: [
            {
              task_id: "T1.1",
              title: "Do thing",
              status: "done",
              role: "architect",
              turn: 1,
            } as any,
            {
              task_id: "T1.2",
              title: "Do other",
              status: "pending",
              role: "worker",
              turn: 1,
            } as any,
          ],
        } as any),
      });
      try {
        const res = await fetch(`${baseUrl}/api/board?projectRoot=/p`);
        assert.equal(res.status, 200);
        const data = (await res.json()) as GetBoardResponse;
        assert.equal(data.sessionId, "s-board");
        assert.equal(data.phase, "running");
        assert.equal(data.progress, "1/2");
        assert.equal(data.tasks.length, 2);
        assert.equal(data.tasks[0]?.id, "T1.1");
        assert.equal(data.tasks[0]?.status, "done");
      } finally {
        await close();
      }
    });

    it("GET /api/plan and POST /api/plan with 409 conflict check", async () => {
      let planState = {
        exists: true,
        content: "# PLAN V1",
        mtimeMs: 1000,
        sha256: "hash-v1",
      };
      let synced = false;

      const { baseUrl, close } = await startTestServer({
        readPlanFile: () => planState,
        writePlanFile: (_path, content) => {
          planState = {
            exists: true,
            content,
            mtimeMs: 2000,
            sha256: "hash-v2",
          };
          return { sha256: "hash-v2", mtimeMs: 2000 };
        },
        withReadOnlyProjectDb: (_root, fn) => {
          return fn({} as any);
        },
        syncPlanFromMd: () => {
          synced = true;
          return { tasksUpdated: 4 };
        },
        resolveAsdPaths: () => ({ planMdPath: "/fake/PLAN.md" } as any),
        getForegroundSessionId: () => "sess-plan",
      });

      try {
        // GET /api/plan
        const resGet = await fetch(`${baseUrl}/api/plan?projectRoot=/p`);
        assert.equal(resGet.status, 200);
        const dataGet = (await resGet.json()) as GetPlanResponse;
        assert.equal(dataGet.content, "# PLAN V1");
        assert.equal(dataGet.sha256, "hash-v1");

        // Conflict check: pass outdated expectedSha256 -> 409 Conflict
        const resConflict = await fetch(`${baseUrl}/api/plan`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            content: "# PLAN V2 CONFLICT",
            expectedSha256: "stale-hash",
          }),
        });
        assert.equal(resConflict.status, 409);
        const dataConflict = (await resConflict.json()) as SavePlanResponse;
        assert.equal(dataConflict.success, false);
        assert.equal(dataConflict.conflict, true);
        assert.equal(dataConflict.sha256, "hash-v1");

        // Successful save: pass matching expectedSha256 -> 200 OK + DB sync
        const resOk = await fetch(`${baseUrl}/api/plan`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            content: "# PLAN V2",
            expectedSha256: "hash-v1",
            syncToDb: true,
          }),
        });
        assert.equal(resOk.status, 200);
        const dataOk = (await resOk.json()) as SavePlanResponse;
        assert.equal(dataOk.success, true);
        assert.equal(dataOk.conflict, false);
        assert.equal(dataOk.sha256, "hash-v2");
        assert.equal(dataOk.tasksUpdated, 4);
        assert.equal(synced, true);
      } finally {
        await close();
      }
    });

    it("POST /api/dispatch and POST /api/debate invoke core handlers", async () => {
      let dispatchCalled = false;
      let debateCalled = false;

      const { baseUrl, close } = await startTestServer({
        handleDispatch: async (input) => {
          dispatchCalled = true;
          return { dispatched: ["T1.1"], message: "Dispatched turn 1" };
        },
        handleDebate: async (input) => {
          debateCalled = true;
          return { verdict: "ACCEPTED", message: "Consensus reached" };
        },
      });

      try {
        const res1 = await fetch(`${baseUrl}/api/dispatch`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-disp",
            turn: 1,
          }),
        });
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as DispatchResponse;
        assert.equal(data1.success, true);
        assert.deepEqual(data1.dispatched, ["T1.1"]);
        assert.equal(dispatchCalled, true);

        const res2 = await fetch(`${baseUrl}/api/debate`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-deb",
            topic: "Design question",
            proposal: "Proposal A",
          }),
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as DebateResponse;
        assert.equal(data2.success, true);
        assert.equal(data2.verdict, "ACCEPTED");
        assert.equal(debateCalled, true);
      } finally {
        await close();
      }
    });
  });

  describe("Worktree, Subagents & Terminal", () => {
    it("GET /api/worktree/diff, POST /api/tasks/approve, POST /api/tasks/reject", async () => {
      let approved = false;
      let rejected = false;

      const { baseUrl, close } = await startTestServer({
        getWorktreeDiff: () => ({
          targetDir: "/p/.asd/wt/1-T1.1",
          diff: "diff --git a b\n+hello",
          changedFiles: [{ path: "a.txt", additions: 1, deletions: 0 }],
          statusShort: "M a.txt",
        }),
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = {
            getAllTasks: () => [{ task_id: "T1.1", session_id: "s-task" }],
          };
          return fn(fakeDb as any);
        },
        approvePendingTask: async () => {
          approved = true;
          return { merged: true };
        },
        rejectPendingTask: () => {
          rejected = true;
        },
      });

      try {
        const res1 = await fetch(
          `${baseUrl}/api/worktree/diff?projectRoot=/p&taskId=T1.1`
        );
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as WorktreeDiffResponse;
        assert.equal(data1.worktreePath, "/p/.asd/wt/1-T1.1");
        assert.equal(data1.files.length, 1);
        assert.equal(data1.files[0]?.path, "a.txt");

        const res2 = await fetch(`${baseUrl}/api/tasks/approve`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ projectRoot: "/p", taskId: "T1.1" }),
        });
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as ApproveTaskResponse;
        assert.equal(data2.success, true);
        assert.equal(approved, true);

        const res3 = await fetch(`${baseUrl}/api/tasks/reject`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            taskId: "T1.1",
            reason: "Bad test",
          }),
        });
        assert.equal(res3.status, 200);
        const data3 = (await res3.json()) as RejectTaskResponse;
        assert.equal(data3.success, true);
        assert.equal(rejected, true);
      } finally {
        await close();
      }
    });

    it("GET /api/subagents, logs, and POST /api/subagent-action", async () => {
      let actionCalled = false;
      const { baseUrl, close } = await startTestServer({
        scanSubagentLogs: () => [
          {
            sessionId: "s-sub",
            subSessionId: "sub-1-T1.1-worker",
            taskId: "T1.1",
            role: "worker",
            harness: "opencode" as any,
            model: "glm-5",
            status: "running",
            startedAt: 1000,
            outputLog: "line 1\nline 2\nline 3\nline 4",
          } as any,
        ],
        runSubagentControlAction: async () => {
          actionCalled = true;
          return { message: "Action kill done" };
        },
      });

      try {
        const res1 = await fetch(`${baseUrl}/api/subagents?projectRoot=/p`);
        assert.equal(res1.status, 200);
        const data1 = (await res1.json()) as GetSubagentsResponse;
        assert.equal(data1.subagents.length, 1);
        assert.equal(data1.subagents[0]?.subSessionId, "sub-1-T1.1-worker");

        const res2 = await fetch(
          `${baseUrl}/api/subagents/sub-1-T1.1-worker/logs?projectRoot=/p&tailLines=2`
        );
        assert.equal(res2.status, 200);
        const data2 = (await res2.json()) as SubagentLogTailResponse;
        assert.equal(data2.subSessionId, "sub-1-T1.1-worker");
        assert.deepEqual(data2.lines, ["line 3", "line 4"]);
        assert.equal(data2.truncated, true);

        const res3 = await fetch(`${baseUrl}/api/subagent-action`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-sub",
            subSessionId: "sub-1-T1.1-worker",
            action: "kill",
          }),
        });
        assert.equal(res3.status, 200);
        const data3 = (await res3.json()) as SubagentActionResponse;
        assert.equal(data3.success, true);
        assert.equal(actionCalled, true);
      } finally {
        await close();
      }
    });

    it("POST /api/terminal/open delegates to SDK and returns command launch spec", async () => {
      const { baseUrl, close } = await startTestServer({
        withReadOnlyProjectDb: (_root, fn) => {
          const fakeDb = {
            getSession: () => ({ harness_session_id: "h-sid", main_model: "m-1" }),
            getActiveDriver: () => null,
          };
          return fn(fakeDb as any);
        },
        buildInteractiveResumeHarnessCommand: (_harness, sid, _model) => ({
          command: "wt.exe",
          args: ["claude", "--resume", sid || ""],
          cliString: `wt.exe claude --resume ${sid || ""}`,
        }),
      });

      try {
        const res = await fetch(`${baseUrl}/api/terminal/open`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: "/p",
            sessionId: "s-term",
            harness: "claude-code",
            terminal: "windows-terminal",
          }),
        });
        assert.equal(res.status, 200);
        const data = (await res.json()) as TerminalOpenResponse;
        assert.equal(data.success, true);
        assert.equal(data.command, "wt.exe");
        assert.deepEqual(data.args, ["claude", "--resume", "h-sid"]);
        assert.equal(data.cliString, "wt.exe claude --resume h-sid");
        assert.equal(data.cwd, "/p");
        assert.equal(data.launched, false);
      } finally {
        await close();
      }
    });
  });

  describe("Harness Commands, Workspace FS List & Slash Prompt Expansion", () => {
    it("GET /api/harness-commands returns builtin, isolated global (homeDir), and project commands/skills across all 5 harnesses", async () => {
      const tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-cmds-home-"));
      const tmpProject = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-cmds-proj-"));

      try {
        // 1. claude-code: global command + project skill
        const ccGlobalCmdDir = path.join(tmpHome, ".claude", "commands");
        fs.mkdirSync(ccGlobalCmdDir, { recursive: true });
        fs.writeFileSync(
          path.join(ccGlobalCmdDir, "deploy-cc.md"),
          "---\ndescription: Deploy via Claude Code\n---\nRun deployment steps."
        );
        const ccProjSkillDir = path.join(tmpProject, ".claude", "skills", "proj-lint");
        fs.mkdirSync(ccProjSkillDir, { recursive: true });
        fs.writeFileSync(
          path.join(ccProjSkillDir, "SKILL.md"),
          "# Project Lint Skill\nRun eslint on src."
        );

        // 2. opencode: global command + project skill
        const ocGlobalCmdDir = path.join(tmpHome, ".config", "opencode", "command");
        fs.mkdirSync(ocGlobalCmdDir, { recursive: true });
        fs.writeFileSync(
          path.join(ocGlobalCmdDir, "oc-audit.md"),
          "---\ndescription: OpenCode security audit\n---\nAudit repository."
        );
        const ocProjCmdDir = path.join(tmpProject, ".opencode", "commands");
        fs.mkdirSync(ocProjCmdDir, { recursive: true });
        fs.writeFileSync(
          path.join(ocProjCmdDir, "oc-local.md"),
          "Local opencode command description\nDetails here."
        );

        // 3. codex: global prompts + project .agents/skills
        const cxGlobalPrompts = path.join(tmpHome, ".codex", "prompts");
        fs.mkdirSync(cxGlobalPrompts, { recursive: true });
        fs.writeFileSync(
          path.join(cxGlobalPrompts, "cx-prompt.md"),
          "---\ndescription: Codex custom prompt\n---\nPrompt body."
        );
        const cxProjSkill = path.join(tmpProject, ".agents", "skills", "cx-agent-skill");
        fs.mkdirSync(cxProjSkill, { recursive: true });
        fs.writeFileSync(
          path.join(cxProjSkill, "SKILL.md"),
          "---\nname: cx-agent-skill\ndescription: Shared agent skill for Codex\n---\nExecute agent skill."
        );

        // 4. antigravity: global builtin skill + project .agent/workflows
        const agBuiltinSkill = path.join(
          tmpHome,
          ".gemini",
          "antigravity-cli",
          "builtin",
          "skills",
          "ag-builtin-helper"
        );
        fs.mkdirSync(agBuiltinSkill, { recursive: true });
        fs.writeFileSync(
          path.join(agBuiltinSkill, "SKILL.md"),
          "---\ndescription: Antigravity builtin skill helper\n---\nHelper instructions."
        );
        const agProjWorkflow = path.join(tmpProject, ".agent", "workflows");
        fs.mkdirSync(agProjWorkflow, { recursive: true });
        fs.writeFileSync(
          path.join(agProjWorkflow, "release-wf.md"),
          "---\ndescription: Release workflow for Antigravity\n---\nStep 1: test."
        );

        // 5. cursor: global skill + project command
        const crGlobalSkill = path.join(tmpHome, ".cursor", "skills", "cr-global-skill");
        fs.mkdirSync(crGlobalSkill, { recursive: true });
        fs.writeFileSync(
          path.join(crGlobalSkill, "SKILL.md"),
          "---\ndescription: Cursor global skill\n---\nDo cursor skill."
        );
        const crProjCmd = path.join(tmpProject, ".cursor", "commands");
        fs.mkdirSync(crProjCmd, { recursive: true });
        fs.writeFileSync(
          path.join(crProjCmd, "cr-fix.md"),
          "Fix cursor lint issues quickly."
        );

        const { baseUrl, close } = await startTestServer(
          {},
          { defaultProjectRoot: tmpProject, homeDir: tmpHome }
        );

        try {
          // Query all harnesses
          const resAll = await fetch(
            `${baseUrl}/api/harness-commands?projectRoot=${encodeURIComponent(tmpProject)}`
          );
          assert.equal(resAll.status, 200);
          const allData = (await resAll.json()) as GetHarnessCommandsResponse;
          assert.ok(allData.commands.length > 20);

          // Verify claude-code filter
          const resCc = await fetch(
            `${baseUrl}/api/harness-commands?harness=claude-code&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const ccData = (await resCc.json()) as GetHarnessCommandsResponse;
          assert.ok(ccData.commands.every((c) => c.harness === "claude-code"));
          assert.ok(
            ccData.commands.some((c) => c.name === "compact" && c.source === "builtin")
          );
          const ccGlobal = ccData.commands.find((c) => c.name === "deploy-cc");
          assert.ok(ccGlobal);
          assert.equal(ccGlobal.source, "global");
          assert.equal(ccGlobal.kind, "command");
          assert.equal(ccGlobal.description, "Deploy via Claude Code");

          const ccProj = ccData.commands.find((c) => c.name === "proj-lint");
          assert.ok(ccProj);
          assert.equal(ccProj.source, "project");
          assert.equal(ccProj.kind, "skill");
          assert.equal(ccProj.description, "Project Lint Skill");

          // Verify opencode filter
          const resOc = await fetch(
            `${baseUrl}/api/harness-commands?harness=opencode&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const ocData = (await resOc.json()) as GetHarnessCommandsResponse;
          assert.ok(ocData.commands.some((c) => c.name === "undo" && c.source === "builtin"));
          assert.ok(
            ocData.commands.some(
              (c) =>
                c.name === "oc-audit" &&
                c.source === "global" &&
                c.description === "OpenCode security audit"
            )
          );
          assert.ok(
            ocData.commands.some(
              (c) =>
                c.name === "oc-local" &&
                c.source === "project" &&
                c.description === "Local opencode command description"
            )
          );

          // Verify codex filter
          const resCx = await fetch(
            `${baseUrl}/api/harness-commands?harness=codex&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const cxData = (await resCx.json()) as GetHarnessCommandsResponse;
          assert.ok(cxData.commands.some((c) => c.name === "diff" && c.source === "builtin"));
          assert.ok(
            cxData.commands.some((c) => c.name === "cx-prompt" && c.source === "global")
          );
          assert.ok(
            cxData.commands.some(
              (c) =>
                c.name === "cx-agent-skill" &&
                c.source === "project" &&
                c.kind === "skill"
            )
          );

          // Verify antigravity filter
          const resAg = await fetch(
            `${baseUrl}/api/harness-commands?harness=antigravity&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const agData = (await resAg.json()) as GetHarnessCommandsResponse;
          assert.ok(agData.commands.some((c) => c.name === "plan" && c.source === "builtin"));
          assert.ok(
            agData.commands.some(
              (c) =>
                c.name === "ag-builtin-helper" &&
                c.source === "global" &&
                c.kind === "skill"
            )
          );
          assert.ok(
            agData.commands.some(
              (c) =>
                c.name === "release-wf" &&
                c.source === "project" &&
                c.kind === "workflow"
            )
          );

          // Verify cursor filter
          const resCr = await fetch(
            `${baseUrl}/api/harness-commands?harness=cursor&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const crData = (await resCr.json()) as GetHarnessCommandsResponse;
          assert.ok(
            crData.commands.some(
              (c) => c.name === "cr-global-skill" && c.source === "global" && c.kind === "skill"
            )
          );
          assert.ok(
            crData.commands.some(
              (c) =>
                c.name === "cr-fix" &&
                c.source === "project" &&
                c.description === "Fix cursor lint issues quickly."
            )
          );
        } finally {
          await close();
        }
      } finally {
        fs.rmSync(tmpHome, { recursive: true, force: true });
        fs.rmSync(tmpProject, { recursive: true, force: true });
      }
    });

    it("GET /api/fs/list supports mode=files (excluding heavy ignored dirs + prefix filter) and mode=dirs navigation", async () => {
      const tmpProject = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-fs-list-"));
      try {
        fs.mkdirSync(path.join(tmpProject, "src", "components"), { recursive: true });
        fs.mkdirSync(path.join(tmpProject, "docs"), { recursive: true });
        fs.mkdirSync(path.join(tmpProject, "node_modules", "pkg"), { recursive: true });
        fs.mkdirSync(path.join(tmpProject, ".git", "objects"), { recursive: true });
        fs.mkdirSync(path.join(tmpProject, "target", "debug"), { recursive: true });
        fs.mkdirSync(path.join(tmpProject, ".asd", "sessions"), { recursive: true });

        fs.writeFileSync(path.join(tmpProject, "src", "index.ts"), "export {};");
        fs.writeFileSync(path.join(tmpProject, "src", "components", "Button.tsx"), "export {};");
        fs.writeFileSync(path.join(tmpProject, "docs", "guide.md"), "# Guide");
        fs.writeFileSync(path.join(tmpProject, "node_modules", "pkg", "index.js"), "");
        fs.writeFileSync(path.join(tmpProject, ".git", "objects", "obj"), "");
        fs.writeFileSync(path.join(tmpProject, "target", "debug", "bin"), "");
        fs.writeFileSync(path.join(tmpProject, ".asd", "sessions", "s.json"), "{}");

        const { baseUrl, close } = await startTestServer(
          {},
          { defaultProjectRoot: tmpProject }
        );

        try {
          // 1. mode=files all
          const resFiles = await fetch(
            `${baseUrl}/api/fs/list?mode=files&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          assert.equal(resFiles.status, 200);
          const dataFiles = (await resFiles.json()) as FsListResponse;
          assert.equal(dataFiles.mode, "files");
          const paths = dataFiles.entries.map((e) => e.path);
          assert.ok(paths.includes("src/index.ts"));
          assert.ok(paths.includes("src/components/Button.tsx"));
          assert.ok(paths.includes("docs/guide.md"));
          assert.ok(!paths.some((p) => p.includes("node_modules")));
          assert.ok(!paths.some((p) => p.includes(".git")));
          assert.ok(!paths.some((p) => p.includes("target")));
          assert.ok(!paths.some((p) => p.includes(".asd")));

          // 2. mode=files with prefix filter
          const resPrefix = await fetch(
            `${baseUrl}/api/fs/list?mode=files&prefix=butt&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const dataPrefix = (await resPrefix.json()) as FsListResponse;
          assert.equal(dataPrefix.entries.length, 1);
          assert.equal(dataPrefix.entries[0]?.path, "src/components/Button.tsx");

          // 3. mode=dirs at root
          const resDirs = await fetch(
            `${baseUrl}/api/fs/list?mode=dirs&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const dataDirs = (await resDirs.json()) as FsListResponse;
          assert.equal(dataDirs.mode, "dirs");
          const dirNames = dataDirs.entries.map((e) => e.name);
          assert.ok(dirNames.includes(".."));
          assert.ok(dirNames.includes("src"));
          assert.ok(dirNames.includes("docs"));
          assert.ok(!dirNames.includes("node_modules"));
          assert.ok(!dirNames.includes(".git"));

          // 4. mode=dirs with partial prefix "src/"
          const resSubDirs = await fetch(
            `${baseUrl}/api/fs/list?mode=dirs&prefix=${encodeURIComponent("src/")}&projectRoot=${encodeURIComponent(tmpProject)}`
          );
          const dataSubDirs = (await resSubDirs.json()) as FsListResponse;
          assert.ok(dataSubDirs.entries.some((e) => e.name === "components" && e.isDir));
        } finally {
          await close();
        }
      } finally {
        fs.rmSync(tmpProject, { recursive: true, force: true });
      }
    });

    it("turns.ts preserves builtin /commands and expands custom .md commands/skills across harnesses", async () => {
      const tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-slash-home-"));
      const tmpProject = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-slash-proj-"));

      try {
        const skillDir = path.join(tmpProject, ".codex", "skills", "my-custom-skill");
        fs.mkdirSync(skillDir, { recursive: true });
        fs.writeFileSync(
          path.join(skillDir, "SKILL.md"),
          "---\nname: my-custom-skill\ndescription: Custom Codex Skill\n---\nAlways reply with CUSTOM_SKILL_OK."
        );

        // Unit check: builtin command is untouched
        const builtinRes = resolveSlashCommandPromptForTurn({
          prompt: "/compact",
          harness: "codex",
          projectRoot: tmpProject,
          homeDir: tmpHome,
        });
        assert.equal(builtinRes.expanded, false);
        assert.equal(builtinRes.prompt, "/compact");

        // Unit check: custom skill is expanded with template + arguments
        const customRes = resolveSlashCommandPromptForTurn({
          prompt: "/my-custom-skill target=src/app.ts",
          harness: "codex",
          projectRoot: tmpProject,
          homeDir: tmpHome,
        });
        assert.equal(customRes.expanded, true);
        assert.ok(customRes.prompt.includes("Always reply with CUSTOM_SKILL_OK."));
        assert.ok(customRes.prompt.includes("target=src/app.ts"));

        // Integration check via POST /api/turn/stream
        const receivedPrompts: string[] = [];
        const { baseUrl, close } = await startTestServer(
          {
            runDriverTurn: async (input) => {
              receivedPrompts.push(input.prompt);
              input.onEvent?.({
                type: "text",
                harness: input.db.getActiveDriver(input.sessionId)?.harness || "codex",
                text: "Executed",
                timestamp: Date.now(),
              });
              return { exitCode: 0, harnessSessionId: "thr-slash-1" } as any;
            },
          },
          { defaultProjectRoot: tmpProject, homeDir: tmpHome }
        );

        try {
          // 1. Send builtin /compact
          const res1 = await fetch(`${baseUrl}/api/turn/stream`, {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Accept: "text/event-stream",
            },
            body: JSON.stringify({
              projectRoot: tmpProject,
              harness: "codex",
              prompt: "/compact",
            }),
          });
          await res1.text();
          assert.equal(receivedPrompts[0], "/compact");

          // 2. Send custom /my-custom-skill with arguments
          const res2 = await fetch(`${baseUrl}/api/turn/stream`, {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Accept: "text/event-stream",
            },
            body: JSON.stringify({
              projectRoot: tmpProject,
              harness: "codex",
              prompt: "/my-custom-skill --fast",
            }),
          });
          await res2.text();
          assert.ok(receivedPrompts[1]?.includes("Always reply with CUSTOM_SKILL_OK."));
          assert.ok(receivedPrompts[1]?.includes("--fast"));
        } finally {
          await close();
        }
      } finally {
        fs.rmSync(tmpHome, { recursive: true, force: true });
        fs.rmSync(tmpProject, { recursive: true, force: true });
      }
    });
  });

  describe("Real Child Process Lifecycle & Handshake", () => {
    it("spawns node index.ts --port 0, parses SidecarReadyInfo, hits /api/health, and exits on stdin EOF", async () => {
      const indexPath = path.resolve(import.meta.dirname, "../src/server/index.ts");
      const proc = child_process.spawn(
        process.execPath,
        ["--no-warnings", indexPath, "--port", "0"],
        {
          stdio: ["pipe", "pipe", "pipe"],
          env: { ...process.env },
        }
      );

      try {
        const readyInfo = await new Promise<SidecarReadyInfo & { baseUrl: string }>((resolve, reject) => {
          let lineBuffer = "";
          proc.stdout.on("data", (chunk: Buffer) => {
            lineBuffer += chunk.toString("utf8");
            const lines = lineBuffer.split("\n");
            for (const line of lines) {
              const trimmed = line.trim();
              if (trimmed.startsWith("{") && trimmed.includes(SIDECAR_READY_PROTOCOL)) {
                try {
                  resolve(JSON.parse(trimmed));
                  return;
                } catch (e) {
                  reject(e);
                  return;
                }
              }
            }
          });
          proc.on("error", reject);
          proc.on("exit", (code) => {
            reject(new Error(`Sidecar process exited unexpectedly with code ${code}`));
          });
        });

        assert.equal(readyInfo.protocol, SIDECAR_READY_PROTOCOL);
        assert.equal(readyInfo.protocolVersion, 1);
        assert.ok(readyInfo.port > 0, "Port must be resolved > 0");
        assert.ok(readyInfo.baseUrl.includes(`:${readyInfo.port}`));
        assert.equal(readyInfo.pid, proc.pid);

        // Hit /api/health on the real spawned sidecar
        const healthRes = await fetch(`${readyInfo.baseUrl}/api/health`);
        assert.equal(healthRes.status, 200);
        const healthData = (await healthRes.json()) as GetHealthResponse;
        assert.equal(healthData.ok, true);
        assert.equal(healthData.version, "2.6.0");
        assert.equal(healthData.pid, proc.pid);

        // Clean exit: close stdin (EOF)
        proc.stdin.end();

        const exitCode = await new Promise<number | null>((resolve) => {
          proc.on("exit", (code) => resolve(code));
        });

        assert.equal(exitCode, 0, "Process must exit cleanly with code 0 on stdin EOF");
      } finally {
        if (!proc.killed) {
          try {
            proc.kill();
          } catch {}
        }
      }
    });
  });
});
