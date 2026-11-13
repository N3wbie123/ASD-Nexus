import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import {
  createOrchestratorServer,
  type OrchestratorDeps,
} from "../src/server/app.ts";
import { normalizeTranscriptMessages } from "../src/server/normalizer.ts";
import type {
  GetBoardResponse,
  GetSessionsResponse,
  GetSubagentsResponse,
  GetTranscriptResponse,
  OrchestratorEvent,
  PromoteSessionResponse,
  SubagentActionResponse,
} from "../src/shared/events.ts";

async function withTestServer(
  deps: OrchestratorDeps,
  fn: (baseUrl: string) => Promise<void>
): Promise<void> {
  const server = createOrchestratorServer({
    defaultProjectRoot: process.cwd(),
    deps,
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  assert.ok(addr && typeof addr === "object");
  const baseUrl = `http://127.0.0.1:${addr.port}`;
  try {
    await fn(baseUrl);
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((err) => (err ? reject(err) : resolve()))
    );
  }
}

function parseSseFrames(rawText: string): OrchestratorEvent[] {
  return rawText
    .split(/\r?\n\r?\n/)
    .map((block) => block.trim())
    .filter(Boolean)
    .flatMap((block) =>
      block
        .split(/\r?\n/)
        .filter((line) => line.startsWith("data:"))
        .map((line) => JSON.parse(line.slice(5).trim()) as OrchestratorEvent)
    );
}

describe("Orchestrator HTTP + SSE Server (M1)", () => {
  it("GET /api/health reports v2.6.0 and M1", async () => {
    await withTestServer({}, async (baseUrl) => {
      const res = await fetch(`${baseUrl}/api/health`);
      assert.equal(res.status, 200);
      const data = (await res.json()) as {
        ok: boolean;
        version: string;
        milestone: string;
      };
      assert.equal(data.ok, true);
      assert.equal(data.version, "2.6.0");
      assert.equal(data.milestone, "M1");
    });
  });

  it("POST /api/turn/stream returns 400 when prompt is missing", async () => {
    await withTestServer({}, async (baseUrl) => {
      const res = await fetch(`${baseUrl}/api/turn/stream`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ harness: "claude-code", prompt: "   " }),
      });
      assert.equal(res.status, 400);
      const body = (await res.json()) as { error: string };
      assert.match(body.error, /prompt/i);
    });
  });

  it("POST /api/turn/stream creates session via MCP handlePlan and streams normalized SSE events without trailing duplicate text", async () => {
    const planCalls: Array<Record<string, unknown>> = [];
    let closedDb = false;

    const deps: OrchestratorDeps = {
      planSession: async (input) => {
        planCalls.push(input as unknown as Record<string, unknown>);
        return {
          sessionId: input.sessionId ?? "asd-20261113-110000-test",
          stateVersion: 1,
          phase: "planning",
          tasks: [],
          planMdPath: ".asd/PLAN.md",
          foreground: false,
          progressBoard: "",
        };
      },
      openDb: () =>
        ({
          close: () => {
            closedDb = true;
          },
        }) as unknown as ReturnType<NonNullable<OrchestratorDeps["openDb"]>>,
      runTurn: async (input) => {
        const sid = "claude-native-uuid-001";
        const initChunk =
          JSON.stringify({
            type: "system",
            subtype: "init",
            session_id: sid,
          }) + "\n";
        input.onOutput?.(initChunk);
        input.onEvent?.({
          type: "status",
          harness: "claude-code",
          harnessSessionId: sid,
          timestamp: Date.now(),
        });
        input.onEvent?.({
          type: "thinking",
          harness: "claude-code",
          thinking: "Planning the response.",
          timestamp: Date.now(),
        });
        input.onEvent?.({
          type: "tool_use",
          harness: "claude-code",
          toolName: "Read",
          toolId: "toolu_1",
          toolInput: JSON.stringify({ file_path: "package.json" }),
          timestamp: Date.now(),
        });
        input.onEvent?.({
          type: "tool_result",
          harness: "claude-code",
          toolId: "toolu_1",
          toolOutput: '{"name":"asd-nexus"}',
          isError: false,
          timestamp: Date.now(),
        });
        input.onEvent?.({
          type: "text",
          harness: "claude-code",
          text: "Final concise answer.",
          timestamp: Date.now(),
        });

        // Simulate trailing Claude result line with identical text + usage
        const resultChunk =
          JSON.stringify({
            type: "result",
            subtype: "success",
            session_id: sid,
            result: "Final concise answer.",
            usage: { input_tokens: 120, output_tokens: 15 },
            total_cost_usd: 0.0025,
          }) + "\n";
        input.onOutput?.(resultChunk);
        input.onEvent?.({
          type: "text",
          harness: "claude-code",
          text: "Final concise answer.",
          harnessSessionId: sid,
          timestamp: Date.now(),
        });

        return {
          runId: "run-test-1",
          driverId: "drv-test-1",
          exitCode: 0,
          timedOut: false,
          harnessSessionId: sid,
          durationMs: 42,
          metaPath: ".asd/sessions/test/meta.json",
        };
      },
    };

    await withTestServer(deps, async (baseUrl) => {
      const res = await fetch(`${baseUrl}/api/turn/stream`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          harness: "claude-code",
          model: "claude-sonnet-5-5",
          prompt: "Inspect package.json",
        }),
      });
      assert.equal(res.status, 200);
      assert.match(res.headers.get("content-type") ?? "", /text\/event-stream/);
      const rawBody = await res.text();
      const events = parseSseFrames(rawBody);

      assert.equal(planCalls.length, 1);
      assert.equal(planCalls[0]?.action, "create");
      assert.equal(closedDb, true);

      const kinds = events.map((e) => e.kind);
      assert.ok(kinds.includes("thinking"));
      assert.ok(kinds.includes("tool_use"));
      assert.ok(kinds.includes("tool_result"));
      assert.ok(kinds.includes("usage"));
      assert.equal(
        events.filter((e) => e.kind === "text").length,
        1,
        "Trailing identical result text must be deduplicated"
      );
      assert.equal(events[0]?.kind, "status");
      assert.equal(events[0]?.status, "running");
      const last = events[events.length - 1];
      assert.equal(last?.kind, "status");
      assert.equal(last?.status, "done");
      assert.equal(
        (last?.raw as { harnessSessionId?: string })?.harnessSessionId,
        "claude-native-uuid-001"
      );
      const usageEv = events.find((e) => e.kind === "usage");
      assert.equal(usageEv?.usage?.inputTokens, 120);
      assert.equal(usageEv?.usage?.outputTokens, 15);
      assert.equal(usageEv?.usage?.costUsd, 0.0025);
    });
  });

  it("POST /api/turn/stream emits error and interrupted status when runTurn rejects", async () => {
    let closedDb = false;
    const deps: OrchestratorDeps = {
      readOnlyDb: (() => ({
        existing: { session_id: "asd-20261113-110000-abcd" },
        active: { harness: "claude-code", model: null },
      })) as unknown as NonNullable<OrchestratorDeps["readOnlyDb"]>,
      openDb: () =>
        ({
          close: () => {
            closedDb = true;
          },
        }) as unknown as ReturnType<NonNullable<OrchestratorDeps["openDb"]>>,
      runTurn: async () => {
        throw new Error("DRIVER_BUSY");
      },
    };

    await withTestServer(deps, async (baseUrl) => {
      const res = await fetch(`${baseUrl}/api/turn/stream`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          sessionId: "asd-20261113-110000-abcd",
          harness: "claude-code",
          prompt: "Hello",
        }),
      });
      assert.equal(res.status, 200);
      const events = parseSseFrames(await res.text());
      assert.equal(closedDb, true);
      const errEv = events.find((e) => e.kind === "error");
      assert.equal(errEv?.error, "DRIVER_BUSY");
      const last = events[events.length - 1];
      assert.equal(last?.kind, "status");
      assert.equal(last?.status, "interrupted");
    });
  });

  it("GET /api/sessions and POST /api/sessions/promote combine CLI discovery and core.db", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-sess-"));
    try {
      const deps: OrchestratorDeps = {
        discoverSessions: () => [
          {
            harness: "claude-code",
            harnessSessionId: "cli-uuid-1",
            projectRoot: tmpDir,
            updatedAt: 2000,
            model: "claude-sonnet-5-5",
            summary: "CLI Session One",
            transcriptPath: "/tmp/cli-uuid-1.jsonl",
            asdSessionId: null,
            startedAt: 1000,
          },
        ],
        readOnlyDb: (() => ({
          sessions: [
            {
              session_id: "asd-20261113-110000-db1",
              project_root: tmpDir,
              main_harness: "claude-code",
              main_model: "claude-sonnet-5-5",
              harness_session_id: null,
              phase: "planning",
              memo: "Fresh ASD Chat",
              created_at: 3000,
              updated_at: 3000,
            },
          ],
          latest: { session_id: "asd-20261113-110000-db1" },
        })) as unknown as NonNullable<OrchestratorDeps["readOnlyDb"]>,
        promoteSession: (opts) => ({
          sessionId: "asd-20261113-110000-prom",
          asdSessionId: "asd-20261113-110000-prom",
          created: true,
          driverId: "drv-1",
          driver: {} as never,
          session: {} as never,
          harness: opts.harness,
          harnessSessionId: opts.harnessSessionId,
          model: opts.model ?? null,
          mainJsonPath: ".asd/sessions/main.json",
        }),
      };

      await withTestServer(deps, async (baseUrl) => {
        const listRes = await fetch(
          `${baseUrl}/api/sessions?projectRoot=${encodeURIComponent(tmpDir)}`
        );
        assert.equal(listRes.status, 200);
        const listed = (await listRes.json()) as GetSessionsResponse;
        assert.equal(listed.activeSessionId, "asd-20261113-110000-db1");
        assert.equal(listed.sessions.length, 2);
        assert.equal(listed.sessions[0]?.asdSessionId, "asd-20261113-110000-db1");
        assert.equal(listed.sessions[0]?.isPromotedToAsd, true);
        assert.equal(listed.sessions[1]?.harnessSessionId, "cli-uuid-1");
        assert.equal(listed.sessions[1]?.isPromotedToAsd, false);

        const promRes = await fetch(`${baseUrl}/api/sessions/promote`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            projectRoot: tmpDir,
            harness: "claude-code",
            harnessSessionId: "cli-uuid-1",
          }),
        });
        assert.equal(promRes.status, 200);
        const promoted = (await promRes.json()) as PromoteSessionResponse;
        assert.equal(promoted.success, true);
        assert.equal(promoted.sessionId, "asd-20261113-110000-prom");
        assert.equal(promoted.created, true);
      });
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("GET /api/transcript/:harness/:id expands 4-category transcript items", async () => {
    const flat = normalizeTranscriptMessages([
      { index: 0, role: "user", category: "user", text: "Hi Claude" },
      {
        index: 1,
        role: "assistant",
        category: "assistant",
        thinking: "Checking files",
        toolUse: [{ id: "t1", name: "Read", input: '{"file_path":"a.ts"}' }],
        toolResult: [{ toolUseId: "t1", name: "Read", output: "ok", isError: false }],
        text: "Done reading.",
      },
    ]);
    assert.deepEqual(
      flat.map((m) => m.category),
      ["user", "thinking", "tool_use", "tool_result", "assistant"]
    );

    const deps: OrchestratorDeps = {
      readTranscript: (harness, sessionId) => ({
        found: true,
        harness,
        sessionId,
        messages: [
          { index: 0, role: "user", category: "user", text: "Hi" },
          {
            index: 1,
            role: "assistant",
            category: "assistant",
            thinking: "Step 1",
            text: "Hello!",
          },
        ],
        hasMore: false,
        truncated: false,
      }),
    };

    await withTestServer(deps, async (baseUrl) => {
      const res = await fetch(`${baseUrl}/api/transcript/claude-code/sess-123`);
      assert.equal(res.status, 200);
      const data = (await res.json()) as GetTranscriptResponse;
      assert.equal(data.harness, "claude-code");
      assert.equal(data.harnessSessionId, "sess-123");
      assert.deepEqual(
        data.messages.map((m) => m.category),
        ["user", "thinking", "assistant"]
      );
    });
  });

  it("GET /api/board, GET /api/subagents, and POST /api/subagent-action map SDK snapshots and actions", async () => {
    const deps: OrchestratorDeps = {
      collectSnapshot: async () =>
        ({
          version: "2.6.0",
          projectRoot: process.cwd(),
          systemLocale: "ko",
          timestamp: Date.now(),
          session: {
            session_id: "asd-20261113-110000-snap",
            phase: "running",
          },
          sessions: [],
          tasks: [
            {
              task_id: "T1.1",
              title: "Setup",
              status: "done",
              role: "worker",
              difficulty: "low",
              turn: 1,
            },
          ],
          allTasks: [],
          debates: [],
          allDebates: [],
          drivers: {},
          subagentLogs: [],
          routingConfig: {} as never,
          hasLocalRouting: false,
          localRoutingPath: "",
          availableModels: {} as never,
        }) as unknown as Awaited<ReturnType<NonNullable<OrchestratorDeps["collectSnapshot"]>>>,
      scanSubagents: () => [
        {
          sessionId: "asd-20261113-110000-snap",
          subSessionId: "sub-1",
          taskId: "T1.1",
          role: "worker",
          harness: "claude-code",
          model: "claude-sonnet-5-5",
          status: "done",
          startedAt: 1700000000000,
          finishedAt: 1700000005000,
          prompt: "test",
          outputLog: "ok",
          artifactsPurged: false,
          dirPath: "/tmp/sub-1",
        },
      ],
      controlSubagent: async (opts) => ({
        ok: true,
        action: opts.action,
        sessionId: opts.sessionId,
        subSessionId: opts.subSessionId,
        status: "killed",
      }),
    };

    await withTestServer(deps, async (baseUrl) => {
      const boardRes = await fetch(`${baseUrl}/api/board`);
      assert.equal(boardRes.status, 200);
      const board = (await boardRes.json()) as GetBoardResponse;
      assert.equal(board.sessionId, "asd-20261113-110000-snap");
      assert.equal(board.progress, "1/1");
      assert.equal(board.tasks[0]?.id, "T1.1");

      const subRes = await fetch(`${baseUrl}/api/subagents`);
      assert.equal(subRes.status, 200);
      const subs = (await subRes.json()) as GetSubagentsResponse;
      assert.equal(subs.subagents.length, 1);
      assert.equal(subs.subagents[0]?.subSessionId, "sub-1");

      const actRes = await fetch(`${baseUrl}/api/subagent-action`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          sessionId: "asd-20261113-110000-snap",
          subSessionId: "sub-1",
          action: "kill",
        }),
      });
      assert.equal(actRes.status, 200);
      const acted = (await actRes.json()) as SubagentActionResponse;
      assert.equal(acted.success, true);
      assert.equal(acted.message, "kill:killed");
    });
  });
});
