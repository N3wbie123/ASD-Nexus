import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { unlinkHarnessSessionFromAsdSession } from "agent-swarm-director";
import { createOrchestratorServer } from "../src/server/app.ts";
import type {
  GetSessionsResponse,
  GetTranscriptResponse,
  OrchestratorEvent,
} from "../src/shared/events.ts";

async function consumeSseTurn(
  baseUrl: string,
  payload: Record<string, unknown>,
  options: {
    signal?: AbortSignal;
    onEvent?: (ev: OrchestratorEvent) => void;
  } = {}
): Promise<OrchestratorEvent[]> {
  const res = await fetch(`${baseUrl}/api/turn/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
    signal: options.signal,
  });

  assert.equal(res.status, 200, `Expected 200 SSE response, got ${res.status}`);
  assert.match(res.headers.get("content-type") ?? "", /text\/event-stream/);
  assert.ok(res.body, "SSE response body must exist");

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  const events: OrchestratorEvent[] = [];
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    let sepIndex: number;
    while ((sepIndex = buffer.indexOf("\n\n")) !== -1) {
      const frame = buffer.slice(0, sepIndex);
      buffer = buffer.slice(sepIndex + 2);
      for (const rawLine of frame.split(/\r?\n/)) {
        const line = rawLine.trim();
        if (!line.startsWith("data:")) continue;
        const json = line.slice(5).trim();
        if (json) {
          const ev = JSON.parse(json) as OrchestratorEvent;
          events.push(ev);
          options.onEvent?.(ev);
        }
      }
    }
  }

  const tail = (buffer + decoder.decode()).trim();
  if (tail) {
    for (const rawLine of tail.split(/\r?\n/)) {
      const line = rawLine.trim();
      if (!line.startsWith("data:")) continue;
      const json = line.slice(5).trim();
      if (json) {
        const ev = JSON.parse(json) as OrchestratorEvent;
        events.push(ev);
        options.onEvent?.(ev);
      }
    }
  }

  return events;
}

function extractHarnessSessionId(events: OrchestratorEvent[]): string | undefined {
  for (let i = events.length - 1; i >= 0; i -= 1) {
    const raw = events[i]?.raw as { harnessSessionId?: unknown } | undefined;
    if (typeof raw?.harnessSessionId === "string" && raw.harnessSessionId.trim()) {
      return raw.harnessSessionId.trim();
    }
  }
  return undefined;
}

describe("Live Claude Code M1 Integration (spawn -> SSE -> --resume -> transcript)", () => {
  it(
    "streams real Claude CLI Turn 1, resumes via --resume on Turn 2, and restores transcript",
    { timeout: 120_000 },
    async (t) => {
      const tempProjectRoot = fs.realpathSync.native(
        fs.mkdtempSync(path.join(os.tmpdir(), "asd-nexus-live-"))
      );
      const server = createOrchestratorServer({
        defaultProjectRoot: tempProjectRoot,
      });

      await new Promise<void>((resolve) =>
        server.listen(0, "127.0.0.1", resolve)
      );
      const addr = server.address();
      assert.ok(addr && typeof addr === "object");
      const baseUrl = `http://127.0.0.1:${addr.port}`;

      let asdSessionId: string | undefined;
      let harnessSessionId: string | undefined;
      const tempClaudeTranscriptDirs = new Set<string>();

      try {
        // Turn 1: Fresh real Claude CLI turn via buildSpawnSpec + runDriverTurn
        const events1 = await consumeSseTurn(
          baseUrl,
          {
            projectRoot: tempProjectRoot,
            harness: "claude-code",
            model: "haiku",
            prompt:
              "Do not invoke tools or read files. Remember this exact secret token for the next turn: NEXUS_SECRET_7492. Reply with ONLY: ACK_7492",
          },
          {
            signal: t.signal,
            onEvent: (ev) => {
              if (!asdSessionId && ev.sessionId) asdSessionId = ev.sessionId;
              const rawSid = (ev.raw as { harnessSessionId?: unknown } | undefined)
                ?.harnessSessionId;
              if (!harnessSessionId && typeof rawSid === "string" && rawSid.trim()) {
                harnessSessionId = rawSid.trim();
              }
            },
          }
        );

        assert.ok(events1.length >= 2, "Expected at least running + done SSE events");
        assert.equal(events1[0]?.kind, "status");
        assert.equal(events1[0]?.status, "running");

        asdSessionId = events1[0]?.sessionId;
        assert.ok(
          asdSessionId && asdSessionId.startsWith("asd-"),
          `Expected valid ASD sessionId, got ${asdSessionId}`
        );

        const last1 = events1[events1.length - 1];
        assert.equal(last1?.kind, "status");
        assert.equal(
          last1?.status,
          "done",
          `Expected Turn 1 done, got events=${JSON.stringify(events1)}`
        );

        const rawLast1 = last1?.raw as
          | { harnessSessionId?: string; exitCode?: number; timedOut?: boolean }
          | undefined;
        assert.equal(rawLast1?.exitCode, 0);
        assert.equal(rawLast1?.timedOut, false);

        harnessSessionId = extractHarnessSessionId(events1);
        assert.ok(
          harnessSessionId && harnessSessionId.length > 0,
          "Turn 1 must yield a non-empty Claude harnessSessionId"
        );

        const text1 = events1
          .filter((e) => e.kind === "text")
          .map((e) => e.text ?? "")
          .join("");
        assert.match(text1, /ACK_7492/);

        // Turn 2: Real --resume on the same ASD session
        const events2 = await consumeSseTurn(
          baseUrl,
          {
            projectRoot: tempProjectRoot,
            sessionId: asdSessionId,
            harness: "claude-code",
            model: "haiku",
            prompt:
              "Do not invoke tools. What exact secret token did I give you in the previous turn? Reply with ONLY that token.",
          },
          { signal: t.signal }
        );

        assert.equal(events2[0]?.kind, "status");
        assert.equal(events2[0]?.status, "running");
        assert.equal(events2[0]?.sessionId, asdSessionId);

        const last2 = events2[events2.length - 1];
        assert.equal(last2?.kind, "status");
        assert.equal(
          last2?.status,
          "done",
          `Expected Turn 2 done, got events=${JSON.stringify(events2)}`
        );

        const rawLast2 = last2?.raw as
          | { harnessSessionId?: string; exitCode?: number; timedOut?: boolean }
          | undefined;
        assert.equal(rawLast2?.exitCode, 0);
        assert.equal(rawLast2?.timedOut, false);

        const harnessSessionId2 = extractHarnessSessionId(events2);
        assert.equal(
          harnessSessionId2,
          harnessSessionId,
          "Turn 2 must preserve the resumed Claude harnessSessionId"
        );

        const text2 = events2
          .filter((e) => e.kind === "text")
          .map((e) => e.text ?? "")
          .join("");
        assert.match(
          text2,
          /NEXUS_SECRET_7492/,
          `Expected Turn 2 --resume reply to recall NEXUS_SECRET_7492, got: ${text2}`
        );

        // Session Discovery Check
        const sessionsRes = await fetch(
          `${baseUrl}/api/sessions?projectRoot=${encodeURIComponent(
            tempProjectRoot
          )}&harness=claude-code`
        );
        assert.equal(sessionsRes.status, 200);
        const sessionsPayload = (await sessionsRes.json()) as GetSessionsResponse;
        const matchedSession = sessionsPayload.sessions.find(
          (s) => s.harnessSessionId === harnessSessionId
        );
        assert.ok(
          matchedSession,
          `Expected Claude session ${harnessSessionId} in GET /api/sessions`
        );
        assert.equal(matchedSession.asdSessionId, asdSessionId);
        assert.equal(matchedSession.isPromotedToAsd, true);

        if (matchedSession.transcriptPath) {
          const claudeProjectsRoot = path.join(os.homedir(), ".claude", "projects");
          const dir = path.dirname(matchedSession.transcriptPath);
          if (
            dir.startsWith(claudeProjectsRoot) &&
            path.basename(dir).includes("asd-nexus-live-")
          ) {
            tempClaudeTranscriptDirs.add(dir);
          }
        }

        // Transcript Restoration Check
        const txQuery = new URLSearchParams({ projectRoot: tempProjectRoot });
        if (matchedSession.transcriptPath) {
          txQuery.set("transcriptPath", matchedSession.transcriptPath);
        }
        const transcriptRes = await fetch(
          `${baseUrl}/api/transcript/claude-code/${encodeURIComponent(
            harnessSessionId
          )}?${txQuery.toString()}`
        );
        assert.equal(transcriptRes.status, 200);
        const transcript = (await transcriptRes.json()) as GetTranscriptResponse;
        assert.equal(transcript.harness, "claude-code");
        assert.equal(transcript.harnessSessionId, harnessSessionId);

        const userTexts = transcript.messages
          .filter((m) => m.category === "user")
          .map((m) => m.text)
          .join("\n");
        const assistantTexts = transcript.messages
          .filter((m) => m.category === "assistant")
          .map((m) => m.text)
          .join("\n");

        assert.match(userTexts, /NEXUS_SECRET_7492/);
        assert.match(assistantTexts, /ACK_7492/);
        assert.match(assistantTexts, /NEXUS_SECRET_7492/);
      } finally {
        if (asdSessionId) {
          try {
            unlinkHarnessSessionFromAsdSession({
              projectRoot: tempProjectRoot,
              asdSessionId,
              harness: "claude-code",
              harnessSessionId,
              deleteAsdSession: true,
            });
          } catch {
            // ignore cleanup failures
          }
        }
        await new Promise<void>((resolve) => server.close(() => resolve()));
        for (const dir of tempClaudeTranscriptDirs) {
          fs.rmSync(dir, { recursive: true, force: true });
        }
        fs.rmSync(tempProjectRoot, { recursive: true, force: true });
      }
    }
  );
});
