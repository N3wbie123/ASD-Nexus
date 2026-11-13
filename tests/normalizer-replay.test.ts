/**
 * Offline replay tests for the 5-harness stream/session/usage normalizer
 * (tests/normalizer-replay.test.ts).
 *
 * Each harness replays captured wire-format NDJSON fixtures from
 * tests/fixtures/<harness>/{stream,error}.ndjson through
 * createHarnessStreamNormalizer and verifies:
 *   - harnessSessionId extraction
 *   - thinking/tool_use/tool_result/text ordering
 *   - duplicate suppression (claude partial-vs-final, antigravity result
 *     response echo, cursor user prompt echo)
 *   - error mapping (kind "error" + interrupted status)
 *   - token/cost usage extraction
 * plus chunk-boundary equivalence (arbitrary split == whole-buffer).
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import {
  createHarnessStreamNormalizer,
  normalizeDriverStreamEvent,
  normalizeTranscriptMessages,
} from "../src/server/normalizer.ts";
import type { NormalizerHarness } from "../src/server/normalizer.ts";
import type { DriverStreamEvent, TranscriptMessage } from "../src/server/sdk.ts";
import type { OrchestratorEvent } from "../src/shared/protocol.ts";

const fixturesDir = path.resolve(import.meta.dirname, "fixtures");
const FIXED_NOW_MS = 1_700_000_000_000;
const FIXED_ISO = new Date(FIXED_NOW_MS).toISOString();

function readFixture(harness: string, name: string): string {
  return fs.readFileSync(path.join(fixturesDir, harness, name), "utf8");
}

function makeNormalizer(harness: NormalizerHarness) {
  return createHarnessStreamNormalizer({
    harness,
    turnId: "turn_test",
    now: () => FIXED_NOW_MS,
  });
}

function replayWhole(harness: NormalizerHarness, content: string): OrchestratorEvent[] {
  const normalizer = makeNormalizer(harness);
  const events = normalizer.pushChunk(content);
  events.push(...normalizer.flush());
  return events;
}

function replayOddChunks(harness: NormalizerHarness, content: string): OrchestratorEvent[] {
  const normalizer = makeNormalizer(harness);
  const events: OrchestratorEvent[] = [];
  for (let i = 0; i < content.length; i += 7) {
    events.push(...normalizer.pushChunk(content.slice(i, i + 7)));
  }
  events.push(...normalizer.flush());
  return events;
}

function kinds(events: OrchestratorEvent[]): string[] {
  return events.map((e) => e.kind);
}

function serialize(events: OrchestratorEvent[]): string {
  return JSON.stringify(events);
}

function requireKind(events: OrchestratorEvent[], kind: string): OrchestratorEvent {
  const found = events.find((e) => e.kind === kind);
  assert.ok(found !== undefined, `expected a "${kind}" event`);
  return found;
}

// ============================================================================
// claude-code
// ============================================================================

describe("claude-code replay", () => {
  const sessionId = "0a1b2c3d-0000-4000-8000-000000000001";

  it("replays stream.ndjson: session, ordering, partial-vs-final dedup, usage", () => {
    const events = replayWhole("claude-code", readFixture("claude-code", "stream.ndjson"));

    assert.deepEqual(kinds(events), [
      "status",
      "thinking",
      "text",
      "text",
      "tool_use",
      "tool_result",
      "text",
      "usage",
      "status",
    ]);

    // harnessSessionId extraction
    assert.equal(events[0]?.sessionId, sessionId);
    for (const event of events) {
      assert.equal(event.sessionId, sessionId, "every event carries the extracted session id");
      assert.equal(event.harness, "claude-code");
      assert.equal(event.turnId, "turn_test");
      assert.equal(event.timestamp, FIXED_ISO);
    }

    // thinking delta streams exactly once (final assistant block deduped)
    const thinkingEvent = requireKind(
      events,
      "thinking"
    );
    assert.equal(thinkingEvent.thinking, "Let me check the directory.");

    // text deltas stream in order; final msg_1 text deduped, msg_2 text kept
    const texts = events
      .filter((e) => e.kind === "text")
      .map((e) => e.text);
    assert.deepEqual(texts, ["Reading the directory", " now.", "The directory listing is ready."]);

    // tool_use emitted once (content_block_stop) with assembled input;
    // the duplicate in the final assistant message is suppressed by tool id
    const toolUse = requireKind(
      events,
      "tool_use"
    );
    assert.deepEqual(toolUse.toolUse, {
      name: "Bash",
      id: "toolu_01ABC",
      input: '{"command":"ls -la"}',
    });

    const toolResult = requireKind(
      events,
      "tool_result"
    );
    assert.deepEqual(toolResult.toolResult, {
      toolUseId: "toolu_01ABC",
      output: "total 8\nfile.txt",
      isError: false,
    });

    // token/cost usage: input + cache_creation + cache_read, + output
    const usage = requireKind(events, "usage");
    assert.deepEqual(usage.usage, {
      inputTokens: 1230,
      outputTokens: 250,
      totalTokens: 1480,
      costUsd: 0.0042,
    });

    const last = events.at(-1);
    assert.equal(last?.kind, "status");
    assert.equal(last?.status, "done");
  });

  it("replays error.ndjson: api_retry ignored, error result mapped", () => {
    const events = replayWhole("claude-code", readFixture("claude-code", "error.ndjson"));

    assert.deepEqual(kinds(events), ["status", "text", "error", "usage", "status"]);
    const errorEvent = requireKind(
      events,
      "error"
    );
    assert.equal(errorEvent.error, "API Error: rate limited");
    const usage = requireKind(events, "usage");
    assert.deepEqual(usage.usage, {
      inputTokens: 10,
      outputTokens: 5,
      totalTokens: 15,
      costUsd: 0.0001,
    });
    assert.equal(events.at(-1)?.status, "interrupted");
  });
});

// ============================================================================
// codex
// ============================================================================

describe("codex replay", () => {
  const threadId = "01a1b12a-1111-4222-8333-444455556666";

  it("replays stream.ndjson: thread id, reasoning/text/tool items, usage", () => {
    const events = replayWhole("codex", readFixture("codex", "stream.ndjson"));

    assert.deepEqual(kinds(events), [
      "status",
      "thinking",
      "text",
      "tool_use",
      "tool_result",
      "tool_use",
      "tool_result",
      "usage",
      "status",
    ]);

    assert.equal(events[0]?.sessionId, threadId);
    for (const event of events) {
      assert.equal(event.sessionId, threadId);
      assert.equal(event.harness, "codex");
    }

    const thinkingEvent = requireKind(
      events,
      "thinking"
    );
    assert.equal(thinkingEvent.thinking, "Considering the task.");

    const textEvent = requireKind(events, "text");
    assert.equal(textEvent.text, "OK");

    const toolUses = events.filter((e) => e.kind === "tool_use");
    assert.deepEqual(toolUses[0]?.toolUse, {
      name: "command_execution",
      id: "item_2",
      input: "git status",
    });
    assert.deepEqual(toolUses[1]?.toolUse, {
      name: "read_file",
      id: "item_3",
      input: '{"path":"a.txt"}',
    });

    const toolResults = events.filter((e) => e.kind === "tool_result");
    assert.deepEqual(toolResults[0]?.toolResult, {
      name: "command_execution",
      toolUseId: "item_2",
      output: "On branch main",
      isError: false,
    });
    assert.deepEqual(toolResults[1]?.toolResult, {
      name: "read_file",
      toolUseId: "item_3",
      output: "file content here",
      isError: false,
    });

    const usage = requireKind(events, "usage");
    assert.deepEqual(usage.usage, {
      inputTokens: 18720,
      outputTokens: 310,
      totalTokens: 19030,
    });
    assert.equal("costUsd" in (usage.usage ?? {}), false, "codex reports no cost");

    assert.equal(events.at(-1)?.status, "done");
  });

  it("replays error.ndjson: error item + turn.failed mapped to error/interrupted", () => {
    const events = replayWhole("codex", readFixture("codex", "error.ndjson"));

    // Both the error item and turn.failed surface as error events, then the
    // turn ends interrupted.
    assert.deepEqual(kinds(events), ["status", "error", "error", "status"]);
    const errorEvents = events.filter((e) => e.kind === "error");
    for (const errorEvent of errorEvents) {
      assert.equal(errorEvent.error, "stream disconnected");
    }
    assert.equal(events.at(-1)?.status, "interrupted");
  });
});

// ============================================================================
// opencode
// ============================================================================

describe("opencode replay", () => {
  const sessionId = "ses_abc123def456789";

  it("replays stream.ndjson: sessionID, reasoning/text/tool parts, tokens+cost", () => {
    const events = replayWhole("opencode", readFixture("opencode", "stream.ndjson"));

    assert.deepEqual(kinds(events), [
      "status",
      "thinking",
      "text",
      "tool_use",
      "tool_result",
      "usage",
      "status",
    ]);

    assert.equal(events[0]?.sessionId, sessionId);
    for (const event of events) {
      assert.equal(event.sessionId, sessionId);
      assert.equal(event.harness, "opencode");
    }

    const thinkingEvent = requireKind(
      events,
      "thinking"
    );
    assert.equal(thinkingEvent.thinking, "thinking internally about the repo");

    const textEvent = requireKind(events, "text");
    assert.equal(textEvent.text, "Checking the project files.");

    const toolUse = requireKind(
      events,
      "tool_use"
    );
    assert.deepEqual(toolUse.toolUse, { name: "bash", id: "call_1", input: '{"command":"ls"}' });

    const toolResult = requireKind(
      events,
      "tool_result"
    );
    assert.deepEqual(toolResult.toolResult, {
      name: "bash",
      toolUseId: "call_1",
      output: "file.txt",
      isError: false,
    });

    const usage = requireKind(events, "usage");
    assert.deepEqual(usage.usage, {
      inputTokens: 420,
      outputTokens: 88,
      totalTokens: 508,
      costUsd: 0.0031,
    });

    assert.equal(events.at(-1)?.status, "done");
  });

  it("replays error.ndjson: provider error mapped to error/interrupted", () => {
    const events = replayWhole("opencode", readFixture("opencode", "error.ndjson"));

    assert.deepEqual(kinds(events), ["status", "error", "status"]);
    const errorEvent = requireKind(
      events,
      "error"
    );
    assert.equal(errorEvent.error, "rate limit exceeded");
    assert.equal(events.at(-1)?.status, "interrupted");
  });
});

// ============================================================================
// antigravity
// ============================================================================

describe("antigravity replay", () => {
  const conversationId = "conv_7f3a2b9c";

  it("replays stream.ndjson: conversation_id, token deltas, result echo dedup", () => {
    const events = replayWhole("antigravity", readFixture("antigravity", "stream.ndjson"));

    // A third "text" from result.response would be the duplicate — its absence
    // in the kind sequence proves the dedup.
    assert.deepEqual(kinds(events), ["status", "text", "text", "status"]);

    assert.equal(events[0]?.sessionId, conversationId);
    for (const event of events) {
      assert.equal(event.sessionId, conversationId);
      assert.equal(event.harness, "antigravity");
    }

    const texts = events
      .filter((e) => e.kind === "text")
      .map((e) => e.text)
      .join("");
    assert.equal(texts, "Analyzing the request.");

    assert.equal(events.at(-1)?.status, "done");
  });

  it("replays error.ndjson: FAILED result mapped to error/interrupted", () => {
    const events = replayWhole("antigravity", readFixture("antigravity", "error.ndjson"));

    assert.deepEqual(kinds(events), ["status", "text", "error", "status"]);
    const texts = events
      .filter((e) => e.kind === "text")
      .map((e) => e.text)
      .join("");
    assert.equal(texts, "Working on it.");
    const errorEvent = requireKind(
      events,
      "error"
    );
    assert.equal(errorEvent.error, "sandbox denied");
    assert.equal(events.at(-1)?.status, "interrupted");
  });
});

// ============================================================================
// cursor
// ============================================================================

describe("cursor replay", () => {
  const sessionId = "chat-9d2e1f0a";

  it("replays stream.ndjson: session id, user echo skip, thinking/tool lifecycle, usage", () => {
    const events = replayWhole("cursor", readFixture("cursor", "stream.ndjson"));

    assert.deepEqual(kinds(events), [
      "status",
      "thinking",
      "tool_use",
      "tool_result",
      "text",
      "usage",
      "status",
    ]);

    assert.equal(events[0]?.sessionId, sessionId);
    for (const event of events) {
      assert.equal(event.sessionId, sessionId);
      assert.equal(event.harness, "cursor");
    }

    // user prompt echo must never surface as text
    const texts = events
      .filter((e) => e.kind === "text")
      .map((e) => e.text);
    assert.deepEqual(texts, ["Here are the files: a.txt, b.txt."]);
    assert.ok(!texts.some((t) => t.includes("list the files")), "user echo skipped");

    const thinkingEvent = requireKind(
      events,
      "thinking"
    );
    assert.equal(thinkingEvent.thinking, "Need a listing first.");

    const toolUse = requireKind(
      events,
      "tool_use"
    );
    assert.deepEqual(toolUse.toolUse, { name: "shell", id: "call_01", input: '{"command":"ls"}' });

    const toolResult = requireKind(
      events,
      "tool_result"
    );
    assert.deepEqual(toolResult.toolResult, {
      name: "shell",
      toolUseId: "call_01",
      output: "a.txt\nb.txt",
      isError: false,
    });

    const usage = requireKind(events, "usage");
    assert.deepEqual(usage.usage, {
      inputTokens: 300,
      outputTokens: 40,
      totalTokens: 340,
      costUsd: 0.0012,
    });

    assert.equal(events.at(-1)?.status, "done");
  });

  it("replays error.ndjson: tool error result + error result mapped", () => {
    const events = replayWhole("cursor", readFixture("cursor", "error.ndjson"));

    assert.deepEqual(kinds(events), ["status", "tool_use", "tool_result", "error", "usage", "status"]);

    const toolResult = requireKind(
      events,
      "tool_result"
    );
    assert.equal(toolResult.toolResult?.isError, true);
    assert.equal(toolResult.toolResult?.output, "permission denied");
    assert.equal(toolResult.toolResult?.toolUseId, "call_9");

    const errorEvent = requireKind(
      events,
      "error"
    );
    assert.equal(errorEvent.error, "permission denied");
    assert.equal(events.at(-1)?.status, "interrupted");
  });
});

// ============================================================================
// Cross-harness chunk-boundary equivalence (NDJSON framing robustness)
// ============================================================================

describe("chunk-boundary equivalence", () => {
  const harnesses: NormalizerHarness[] = [
    "claude-code",
    "codex",
    "opencode",
    "antigravity",
    "cursor",
  ];

  for (const harness of harnesses) {
    for (const fixtureName of ["stream.ndjson", "error.ndjson"]) {
      it(`${harness}/${fixtureName}: 7-byte chunks replay identically to whole buffer`, () => {
        const content = readFixture(harness, fixtureName);
        const whole = serialize(replayWhole(harness, content));
        const odd = serialize(replayOddChunks(harness, content));
        assert.equal(odd, whole, "chunk boundaries must not change normalized output");
      });
    }
  }

  it("ignores banner noise and torn JSON lines without emitting errors", () => {
    const normalizer = makeNormalizer("claude-code");
    const noise = "ASD banner output\n{\"type\":\"assistant\", TORN LINE\n";
    const events = normalizer.pushChunk(noise);
    events.push(...normalizer.flush());
    assert.equal(events.length, 0);
  });
});

// ============================================================================
// normalizeDriverStreamEvent — Core SDK parsed event mapping
// ============================================================================

describe("normalizeDriverStreamEvent", () => {
  const baseEvent: Omit<DriverStreamEvent, "type" | "timestamp"> = {
    harness: "claude-code",
  };

  function withType(type: DriverStreamEvent["type"], extra: Partial<DriverStreamEvent> = {}) {
    return { ...baseEvent, type, timestamp: FIXED_NOW_MS, ...extra } as DriverStreamEvent;
  }

  it("maps thinking with session id and ISO timestamp", () => {
    const event = normalizeDriverStreamEvent(
      withType("thinking", { thinking: "deep thought", harnessSessionId: "s-1" })
    );
    assert.deepEqual(event, {
      kind: "thinking",
      thinking: "deep thought",
      harness: "claude-code",
      sessionId: "s-1",
      timestamp: FIXED_ISO,
      turnId: undefined,
    });
  });

  it("maps text and falls back thinking->text", () => {
    const textEvent = normalizeDriverStreamEvent(withType("text", { text: "hello" }));
    assert.equal(textEvent?.kind, "text");
    assert.equal(textEvent?.text, "hello");

    const fallback = normalizeDriverStreamEvent(withType("thinking", { text: "as text" }));
    assert.equal(fallback?.kind, "thinking");
    assert.equal(fallback?.thinking, "as text");
  });

  it("maps tool_use and tool_result payloads", () => {
    const use = normalizeDriverStreamEvent(
      withType("tool_use", {
        toolName: "Bash",
        toolId: "t1",
        toolInput: '{"command":"ls"}',
        harnessSessionId: "s-2",
      })
    );
    assert.equal(use?.kind, "tool_use");
    assert.deepEqual(use?.toolUse, { name: "Bash", id: "t1", input: '{"command":"ls"}' });
    assert.equal(use?.sessionId, "s-2");

    const result = normalizeDriverStreamEvent(
      withType("tool_result", {
        toolName: "Bash",
        toolId: "t1",
        toolOutput: "ok",
        isError: true,
      })
    );
    assert.equal(result?.kind, "tool_result");
    assert.deepEqual(result?.toolResult, {
      name: "Bash",
      toolUseId: "t1",
      output: "ok",
      isError: true,
    });
  });

  it("maps status to running and drops raw events", () => {
    const status = normalizeDriverStreamEvent(withType("status", { text: "started" }));
    assert.equal(status?.kind, "status");
    assert.equal(status?.status, "running");
    assert.equal(status?.text, "started");

    assert.equal(normalizeDriverStreamEvent(withType("raw", { text: "noise" })), null);
  });

  it("attaches turnId when provided", () => {
    const event = normalizeDriverStreamEvent(withType("text", { text: "hi" }), {
      turnId: "turn-9",
    });
    assert.equal(event?.turnId, "turn-9");
  });
});

// ============================================================================
// normalizeTranscriptMessages — 4-category restoration
// ============================================================================

describe("normalizeTranscriptMessages", () => {
  it("restores user/assistant/system categories with roles", () => {
    const messages: TranscriptMessage[] = [
      { index: 0, role: "user", category: "user", text: "do the thing" },
      { index: 1, role: "assistant", category: "assistant", text: "done" },
      { index: 2, role: "system", category: "system", text: "context notice" },
    ];
    const normalized = normalizeTranscriptMessages(messages);
    assert.deepEqual(
      normalized.map((m) => ({ role: m.role, category: m.category, text: m.text })),
      [
        { role: "user", category: "user", text: "do the thing" },
        { role: "assistant", category: "assistant", text: "done" },
        { role: "system", category: "system", text: "context notice" },
      ]
    );
  });

  it("maps thinking category with thinking field preserved", () => {
    const messages: TranscriptMessage[] = [
      { index: 0, role: "assistant", category: "thinking", text: "", thinking: "reasoning here" },
    ];
    const normalized = normalizeTranscriptMessages(messages);
    assert.equal(normalized.length, 1);
    assert.equal(normalized[0]?.role, "assistant");
    assert.equal(normalized[0]?.category, "thinking");
    assert.equal(normalized[0]?.thinking, "reasoning here");
  });

  it("splits multi-item tool_use/tool_result arrays into one message per item", () => {
    const messages: TranscriptMessage[] = [
      {
        index: 0,
        role: "tool",
        category: "tool_use",
        text: "",
        toolUse: [
          { id: "t1", name: "Bash", input: '{"command":"ls"}' },
          { id: "t2", name: "Read", input: '{"path":"a.txt"}' },
        ],
      },
      {
        index: 1,
        role: "tool",
        category: "tool_result",
        text: "out",
        toolResult: [{ toolUseId: "t1", name: "Bash", output: "files", isError: false }],
      },
    ];
    const normalized = normalizeTranscriptMessages(messages);
    assert.equal(normalized.length, 3);

    assert.equal(normalized[0]?.category, "tool_use");
    assert.deepEqual(normalized[0]?.toolUse, { name: "Bash", id: "t1", input: '{"command":"ls"}' });
    assert.equal(normalized[1]?.category, "tool_use");
    assert.deepEqual(normalized[1]?.toolUse, { name: "Read", id: "t2", input: '{"path":"a.txt"}' });

    assert.equal(normalized[2]?.category, "tool_result");
    assert.equal(normalized[2]?.text, "files");
    assert.deepEqual(normalized[2]?.toolResult, {
      name: "Bash",
      toolUseId: "t1",
      output: "files",
      isError: false,
    });
  });

  it("splits merged assistant messages (mergeToolResults mode) into ordered view", () => {
    const messages: TranscriptMessage[] = [
      {
        index: 0,
        role: "assistant",
        category: "assistant",
        text: "Running a check.",
        toolUse: [{ id: "t9", name: "Bash", input: "npm test" }],
        toolResult: [{ toolUseId: "t9", name: "Bash", output: "all green", isError: false }],
      },
    ];
    const normalized = normalizeTranscriptMessages(messages);
    assert.deepEqual(normalized.map((m) => m.category), [
      "assistant",
      "tool_use",
      "tool_result",
    ]);
    assert.equal(normalized[0]?.text, "Running a check.");
    assert.equal(normalized[1]?.role, "tool");
    assert.equal(normalized[2]?.toolResult?.output, "all green");
  });

  it("infers missing category from role and attached tool items", () => {
    const messages: TranscriptMessage[] = [
      { index: 0, role: "user", text: "hi" },
      { index: 1, role: "tool", text: "", toolUse: [{ name: "Bash" }] },
    ];
    const normalized = normalizeTranscriptMessages(messages);
    assert.equal(normalized[0]?.category, "user");
    assert.equal(normalized[1]?.category, "tool_use");
    assert.equal(normalized[1]?.role, "tool");
  });
});
