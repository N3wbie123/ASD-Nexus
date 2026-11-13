import { describe, it, after } from "node:test";
import assert from "node:assert/strict";
import * as child_process from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import type { AddressInfo } from "node:net";
import { createOrchestratorApp } from "../src/server/app.ts";
import type {
  GetSessionsResponse,
  GetTranscriptResponse,
  OrchestratorEvent,
} from "../src/shared/protocol.ts";

interface HarnessE2ECase {
  harness: "claude-code" | "codex" | "opencode" | "antigravity" | "cursor";
  model: string;
}

const HARNESS_CASES: HarnessE2ECase[] = [
  {
    harness: "claude-code",
    model: process.env.ASD_E2E_MODEL_CLAUDE || "haiku",
  },
  {
    harness: "codex",
    model: process.env.ASD_E2E_MODEL_CODEX || "gpt-6-luna",
  },
  {
    harness: "opencode",
    model: process.env.ASD_E2E_MODEL_OPENCODE || "opencode/glm-5.3-flash-free",
  },
  {
    harness: "antigravity",
    model: process.env.ASD_E2E_MODEL_ANTIGRAVITY || "gemini-3.8-flash-low",
  },
  {
    harness: "cursor",
    model: process.env.ASD_E2E_MODEL_CURSOR || "default",
  },
];

const VALID_CATEGORIES = new Set([
  "user",
  "assistant",
  "thinking",
  "tool_use",
  "tool_result",
  "system",
]);

async function postSseTurn(
  baseUrl: string,
  body: Record<string, unknown>
): Promise<OrchestratorEvent[]> {
  const res = await fetch(`${baseUrl}/api/turn/stream`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream",
    },
    body: JSON.stringify(body),
  });
  assert.equal(res.status, 200, "POST /api/turn/stream must return HTTP 200");
  assert.ok(
    res.headers.get("content-type")?.includes("text/event-stream"),
    "POST /api/turn/stream must return text/event-stream"
  );

  const rawText = await res.text();
  return rawText
    .split("\n\n")
    .map((chunk) => chunk.trim())
    .filter((line) => line.startsWith("data: "))
    .map((line) => JSON.parse(line.slice(6)) as OrchestratorEvent);
}

async function fetchTranscriptWithSettleRetry(
  url: string
): Promise<GetTranscriptResponse> {
  let latest: GetTranscriptResponse | null = null;
  for (let attempt = 0; attempt < 8; attempt++) {
    const res = await fetch(url);
    assert.equal(res.status, 200);
    const data = (await res.json()) as GetTranscriptResponse;
    latest = data;
    const cats = new Set(data.messages.map((m) => m.category));
    if (
      data.messages.length >= 2 &&
      cats.has("user") &&
      cats.has("assistant")
    ) {
      return data;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  return latest!;
}

describe("Live 5-Harness CLI E2E Verification", { timeout: 600_000 }, () => {
  const results: Array<Record<string, unknown>> = [];

  after(() => {
    if (results.length === HARNESS_CASES.length) {
      const evidenceDir = path.resolve(process.cwd(), "docs", "evidence");
      fs.mkdirSync(evidenceDir, { recursive: true });
      fs.writeFileSync(
        path.join(evidenceDir, "e2e-5harness-summary.json"),
        JSON.stringify(
          {
            generatedAt: new Date().toISOString(),
            totalHarnesses: results.length,
            allPassed: results.every((r) => r.passed === true),
            harnesses: results,
          },
          null,
          2
        ) + "\n",
        "utf8"
      );
    }
  });

  for (const testCase of HARNESS_CASES) {
    it(
      `${testCase.harness} (${testCase.model}): Turn 1 SSE -> Turn 2 resume -> session discovery -> normalized transcript`,
      { timeout: 240_000 },
      async () => {
        const tmpProj = fs.mkdtempSync(
          path.join(os.tmpdir(), `asd-e2e-${testCase.harness}-`)
        );
        child_process.execFileSync("git", ["init", "-q"], { cwd: tmpProj });

        const server = createOrchestratorApp({}, { defaultProjectRoot: tmpProj });
        await new Promise<void>((resolve) =>
          server.listen(0, "127.0.0.1", () => resolve())
        );
        const port = (server.address() as AddressInfo).port;
        const baseUrl = `http://127.0.0.1:${port}`;

        const startedAt = Date.now();
        let asdSessionId: string | undefined;

        try {
          // 1. Turn 1: fresh turn through HTTP/SSE -> real CLI -> normalizer
          const t1Events = await postSseTurn(baseUrl, {
            projectRoot: tmpProj,
            harness: testCase.harness,
            model: testCase.model,
            prompt:
              "Remember the calibration code AMBER-42. Reply with only OK.",
          });

          assert.equal(t1Events[0]?.kind, "status");
          assert.equal(t1Events[0]?.status, "running");
          asdSessionId = t1Events[0]?.sessionId;
          assert.ok(
            asdSessionId && asdSessionId.startsWith("asd-"),
            `${testCase.harness}: expected asd-* sessionId on Turn 1 start`
          );

          const t1Errors = t1Events
            .filter((e) => e.kind === "error")
            .map((e) => e.error);
          assert.deepEqual(
            t1Errors,
            [],
            `${testCase.harness}: Turn 1 emitted error events: ${JSON.stringify(t1Errors)}`
          );

          const t1Done = t1Events.at(-1);
          assert.equal(t1Done?.kind, "status");
          assert.equal(t1Done?.status, "done");

          const harnessSessionId =
            (t1Done?.raw as { harnessSessionId?: string } | undefined)
              ?.harnessSessionId ||
            t1Events.find((e) => e.sessionId && e.sessionId !== asdSessionId)
              ?.sessionId;
          assert.ok(
            harnessSessionId && harnessSessionId.length > 0,
            `${testCase.harness}: must extract harnessSessionId`
          );

          const t1Text = t1Events
            .filter((e) => e.kind === "text")
            .map((e) => e.text || "")
            .join("")
            .trim();
          assert.ok(
            t1Text.length > 0,
            `${testCase.harness}: Turn 1 must stream assistant text`
          );

          // 2. Turn 2: resume on the same ASD session and verify memory retention
          const t2Events = await postSseTurn(baseUrl, {
            projectRoot: tmpProj,
            sessionId: asdSessionId,
            harness: testCase.harness,
            model: testCase.model,
            prompt:
              "What calibration code did I ask you to remember? Reply with only the code.",
          });

          const t2Errors = t2Events
            .filter((e) => e.kind === "error")
            .map((e) => e.error);
          assert.deepEqual(
            t2Errors,
            [],
            `${testCase.harness}: Turn 2 emitted error events: ${JSON.stringify(t2Errors)}`
          );
          assert.equal(t2Events.at(-1)?.kind, "status");
          assert.equal(t2Events.at(-1)?.status, "done");

          const t2Text = t2Events
            .filter((e) => e.kind === "text")
            .map((e) => e.text || "")
            .join("")
            .trim();
          assert.ok(
            t2Text.includes("AMBER-42"),
            `${testCase.harness}: Turn 2 resume expected "AMBER-42", got ${JSON.stringify(t2Text)}`
          );

          // 3. Session Discovery via GET /api/sessions
          const sessRes = await fetch(
            `${baseUrl}/api/sessions?projectRoot=${encodeURIComponent(tmpProj)}&syncLocalTranscripts=true`
          );
          assert.equal(sessRes.status, 200);
          const sessData = (await sessRes.json()) as GetSessionsResponse;
          const discovered = sessData.sessions.find(
            (s) =>
              s.harness === testCase.harness &&
              s.harnessSessionId === harnessSessionId
          );
          assert.ok(
            discovered,
            `${testCase.harness}: session ${harnessSessionId} not found in /api/sessions`
          );

          // 4. Transcript Restoration via GET /api/transcript/:harness/:id
          const tpParam = discovered.transcriptPath
            ? `&transcriptPath=${encodeURIComponent(discovered.transcriptPath)}`
            : "";
          const transcript = await fetchTranscriptWithSettleRetry(
            `${baseUrl}/api/transcript/${testCase.harness}/${encodeURIComponent(harnessSessionId)}?projectRoot=${encodeURIComponent(tmpProj)}${tpParam}`
          );
          assert.equal(
            transcript.error,
            undefined,
            `${testCase.harness}: transcript error: ${transcript.error}`
          );
          assert.ok(
            transcript.messages.length >= 2,
            `${testCase.harness}: expected >= 2 transcript messages, got ${transcript.messages.length}`
          );
          const categories = new Set(
            transcript.messages.map((m) => m.category)
          );
          assert.ok(
            categories.has("user") && categories.has("assistant"),
            `${testCase.harness}: expected user & assistant categories, got ${JSON.stringify([...categories])}`
          );
          for (const msg of transcript.messages) {
            if (msg.category) {
              assert.ok(
                VALID_CATEGORIES.has(msg.category),
                `Unexpected category: ${msg.category}`
              );
            }
          }

          results.push({
            harness: testCase.harness,
            model: testCase.model,
            durationMs: Date.now() - startedAt,
            asdSessionId,
            harnessSessionId,
            turn1EventKinds: [...new Set(t1Events.map((e) => e.kind))],
            turn2Text: t2Text,
            discovered: true,
            transcriptMessages: transcript.messages.length,
            transcriptCategories: [...categories],
            passed: true,
          });
        } finally {
          if (asdSessionId) {
            try {
              await fetch(
                `${baseUrl}/api/sessions/${encodeURIComponent(asdSessionId)}?projectRoot=${encodeURIComponent(tmpProj)}`,
                { method: "DELETE" }
              );
            } catch {
              // best-effort cleanup
            }
          }
          await new Promise<void>((resolve) => server.close(() => resolve()));
          try {
            fs.rmSync(tmpProj, {
              recursive: true,
              force: true,
              maxRetries: 15,
              retryDelay: 250,
            });
          } catch {
            // Windows subprocesses (e.g. opencode LSP) can briefly lock temp dirs.
          }
        }
      }
    );
  }
});
