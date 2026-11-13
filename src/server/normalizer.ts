/**
 * ASD-Nexus 5-Harness Stream/Session/Usage Normalizer (src/server/normalizer.ts)
 *
 * Bridges harness wire formats (and the Core SDK's DriverStreamEvent) to the
 * shared OrchestratorEvent contract (src/shared/protocol.ts). Pure TypeScript,
 * zero Node imports, zero runtime dependencies.
 *
 * Two ingestion paths:
 *  1. createHarnessStreamNormalizer — stateful raw NDJSON replay of child
 *     stdout across all 5 harnesses. Owns harnessSessionId extraction,
 *     thinking/tool_use/tool_result/text ordering, duplicate suppression
 *     (claude partial-vs-final blocks, antigravity result.response echo,
 *     cursor user prompt echo), error mapping, and token/cost usage.
 *  2. normalizeDriverStreamEvent — 1:1 mapping of a Core SDK parsed
 *     DriverStreamEvent into an OrchestratorEvent (usage is absent there; the
 *     raw-NDJSON path is the only usage source).
 *
 * Plus normalizeTranscriptMessages for 4-category history restoration
 * (Core TranscriptMessage[] -> protocol NormalizedTranscriptMessage[]).
 */

import type { DriverStreamEvent, TranscriptMessage } from "./sdk.ts";
import type {
  NormalizedTranscriptMessage,
  OrchestratorEvent,
  OrchestratorEventKind,
  OrchestratorTurnStatus,
  TokenUsagePayload,
  TranscriptCategory,
} from "../shared/protocol.ts";

// ============================================================================
// Shared tolerant JSON helpers
// ============================================================================

type Rec = Record<string, unknown>;

function isRec(v: unknown): v is Rec {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function asRec(v: unknown): Rec {
  return isRec(v) ? v : {};
}

function str(v: unknown): string | undefined {
  return typeof v === "string" ? v : undefined;
}

function num(v: unknown): number | undefined {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string" && v.trim() !== "") {
    const parsed = Number(v);
    if (Number.isFinite(parsed)) return parsed;
  }
  return undefined;
}

function arr(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

/** JSON.stringify for tool input/argument payloads; strings pass through. */
function inputString(v: unknown): string | undefined {
  if (v === undefined || v === null) return undefined;
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

/**
 * Renders a tool result content payload to plain text. Claude/Cursor results
 * may be a plain string, an array of {type:"text",text} blocks, or arbitrary
 * JSON — the latter is stringified losslessly.
 */
function resultOutput(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "string") return v;
  if (Array.isArray(v)) {
    const parts: string[] = [];
    for (const block of v) {
      const rec = asRec(block);
      const text = str(rec.text);
      parts.push(text ?? JSON.stringify(block));
    }
    return parts.join("\n");
  }
  if (isRec(v)) {
    const direct = str(v.output) ?? str(v.text) ?? str(v.content) ?? str(v.message);
    if (direct !== undefined) return direct;
  }
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

// ============================================================================
// Harness stream normalizer (raw NDJSON -> OrchestratorEvent)
// ============================================================================

export const NORMALIZER_HARNESSES = [
  "claude-code",
  "codex",
  "opencode",
  "antigravity",
  "cursor",
] as const;

export type NormalizerHarness = (typeof NORMALIZER_HARNESSES)[number];

export interface HarnessStreamNormalizerOptions {
  harness: NormalizerHarness;
  /** Attached to every emitted event. */
  turnId?: string;
  /** Overridable clock (tests). Defaults to Date.now. */
  now?: () => number;
}

export interface HarnessStreamNormalizer {
  /** Feed one complete NDJSON line (no trailing newline required). */
  pushLine(line: string): OrchestratorEvent[];
  /** Feed an arbitrary chunk; buffers partial lines across calls. */
  pushChunk(chunk: string | Buffer): OrchestratorEvent[];
  /** Drain any buffered partial line / dangling tool blocks at stream end. */
  flush(): OrchestratorEvent[];
}

/**
 * Builds the harness-agnostic stream normalizer. One instance per turn:
 * dedup state (claude partials, antigravity echo, cursor echo) lives here.
 */
export function createHarnessStreamNormalizer(
  options: HarnessStreamNormalizerOptions
): HarnessStreamNormalizer {
  const harness = options.harness;
  const turnId = options.turnId;
  const getNow = options.now ?? (() => Date.now());

  let sessionId: string | undefined;
  let lineBuffer = "";

  function ev(
    kind: OrchestratorEventKind,
    extra: Partial<OrchestratorEvent> = {}
  ): OrchestratorEvent {
    const event: Partial<OrchestratorEvent> & { kind: OrchestratorEventKind } = {
      kind,
      turnId,
      harness,
      timestamp: new Date(getNow()).toISOString(),
      ...extra,
    };
    if (sessionId !== undefined) event.sessionId = sessionId;
    return event as OrchestratorEvent;
  }

  function statusEvent(status: OrchestratorTurnStatus, text?: string): OrchestratorEvent {
    return ev("status", text === undefined ? { status } : { status, text });
  }

  function errorEvent(message: string): OrchestratorEvent {
    return ev("error", { error: message });
  }

  function usageEvent(usage: TokenUsagePayload): OrchestratorEvent {
    return ev("usage", { usage });
  }

  function setSession(id: string | undefined): void {
    if (typeof id === "string" && id.length > 0) sessionId = id;
  }

  // --------------------------------------------------------------------------
  // claude-code: system/init -> stream_event deltas -> assistant -> user
  //              (tool_result) -> result (usage/cost/session)
  // Dedup: with --include-partial-messages the final `assistant` message
  // duplicates the streamed deltas; final text/thinking blocks are suppressed
  // when deltas were emitted for the current message. tool_use blocks are
  // deduped by tool id (streamed at content_block_stop with assembled input).
  // --------------------------------------------------------------------------
  const claudeToolBlocks = new Map<number, { id?: string; name: string; partialJson: string }>();
  const claudeEmittedToolIds = new Set<string>();
  let claudeDeltasSinceAssistant = false;

  function claudeToolUseFromAssistant(block: Rec): OrchestratorEvent | null {
    const id = str(block.id);
    if (id !== undefined && claudeEmittedToolIds.has(id)) return null;
    if (id !== undefined) claudeEmittedToolIds.add(id);
    return ev("tool_use", {
      toolUse: {
        name: str(block.name) ?? "unknown",
        id,
        input: inputString(block.input),
      },
    });
  }

  function handleClaudeLine(obj: Rec): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    setSession(str(obj.session_id));

    const lineType = str(obj.type) ?? "";
    if (lineType === "system") {
      const subtype = str(obj.subtype) ?? "";
      if (subtype === "init") {
        out.push(statusEvent("running"));
      }
      return out;
    }

    if (lineType === "stream_event") {
      const event = asRec(obj.event);
      const eventType = str(event.type) ?? "";
      if (eventType === "content_block_start") {
        const block = asRec(event.content_block);
        if (str(block.type) === "tool_use") {
          claudeToolBlocks.set(num(event.index) ?? 0, {
            id: str(block.id),
            name: str(block.name) ?? "unknown",
            partialJson: "",
          });
        }
        return out;
      }
      if (eventType === "content_block_delta") {
        const delta = asRec(event.delta);
        const deltaType = str(delta.type) ?? "";
        const index = num(event.index) ?? 0;
        if (deltaType === "text_delta") {
          const text = str(delta.text);
          if (text !== undefined && text.length > 0) {
            claudeDeltasSinceAssistant = true;
            out.push(ev("text", { text }));
          }
        } else if (deltaType === "thinking_delta") {
          const thinking = str(delta.thinking);
          if (thinking !== undefined && thinking.length > 0) {
            claudeDeltasSinceAssistant = true;
            out.push(ev("thinking", { thinking }));
          }
        } else if (deltaType === "input_json_delta") {
          const toolBlock = claudeToolBlocks.get(index);
          const partial = str(delta.partial_json);
          if (toolBlock !== undefined && partial !== undefined) {
            toolBlock.partialJson += partial;
          }
        }
        return out;
      }
      if (eventType === "content_block_stop") {
        const toolBlock = claudeToolBlocks.get(num(event.index) ?? 0);
        if (toolBlock !== undefined) {
          claudeToolBlocks.delete(num(event.index) ?? 0);
          if (toolBlock.id !== undefined) claudeEmittedToolIds.add(toolBlock.id);
          out.push(
            ev("tool_use", {
              toolUse: {
                name: toolBlock.name,
                id: toolBlock.id,
                input: toolBlock.partialJson.length > 0 ? toolBlock.partialJson : undefined,
              },
            })
          );
        }
        return out;
      }
      return out;
    }

    if (lineType === "assistant") {
      const message = asRec(obj.message);
      for (const rawBlock of arr(message.content)) {
        const block = asRec(rawBlock);
        const blockType = str(block.type) ?? "";
        if (blockType === "text") {
          if (!claudeDeltasSinceAssistant) {
            const text = str(block.text);
            if (text !== undefined && text.length > 0) out.push(ev("text", { text }));
          }
        } else if (blockType === "thinking") {
          if (!claudeDeltasSinceAssistant) {
            const thinking = str(block.thinking);
            if (thinking !== undefined && thinking.length > 0) {
              out.push(ev("thinking", { thinking }));
            }
          }
        } else if (blockType === "tool_use") {
          const toolEvent = claudeToolUseFromAssistant(block);
          if (toolEvent !== null) out.push(toolEvent);
        }
      }
      claudeDeltasSinceAssistant = false;
      return out;
    }

    if (lineType === "user") {
      const message = asRec(obj.message);
      for (const rawBlock of arr(message.content)) {
        const block = asRec(rawBlock);
        if (str(block.type) === "tool_result") {
          out.push(
            ev("tool_result", {
              toolResult: {
                toolUseId: str(block.tool_use_id),
                output: resultOutput(block.content),
                isError: block.is_error === true,
              },
            })
          );
        }
      }
      return out;
    }

    if (lineType === "result") {
      const isError = obj.is_error === true || (str(obj.subtype) ?? "success") !== "success";
      const usageRaw = asRec(obj.usage);
      const inputTokens =
        (num(usageRaw.input_tokens) ?? 0) +
        (num(usageRaw.cache_creation_input_tokens) ?? 0) +
        (num(usageRaw.cache_read_input_tokens) ?? 0);
      const outputTokens = num(usageRaw.output_tokens) ?? 0;
      const usage: TokenUsagePayload = {
        inputTokens,
        outputTokens,
        totalTokens: inputTokens + outputTokens,
        costUsd: num(obj.total_cost_usd) ?? num(obj.cost_usd),
      };
      claudeDeltasSinceAssistant = false;
      claudeEmittedToolIds.clear();
      if (isError) {
        const message =
          str(obj.result) ??
          str(asRec(obj.error).message) ??
          str(obj.subtype) ??
          "claude turn failed";
        out.push(errorEvent(message));
        out.push(usageEvent(usage));
        out.push(statusEvent("interrupted"));
      } else {
        out.push(usageEvent(usage));
        out.push(statusEvent("done"));
      }
      return out;
    }

    return out;
  }

  function flushClaude(): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    for (const toolBlock of claudeToolBlocks.values()) {
      if (toolBlock.id !== undefined) claudeEmittedToolIds.add(toolBlock.id);
      out.push(
        ev("tool_use", {
          toolUse: {
            name: toolBlock.name,
            id: toolBlock.id,
            input: toolBlock.partialJson.length > 0 ? toolBlock.partialJson : undefined,
          },
        })
      );
    }
    claudeToolBlocks.clear();
    return out;
  }

  // --------------------------------------------------------------------------
  // codex: thread.started(thread_id) -> turn.started -> item.started/completed
  //        (agent_message/reasoning/command_execution/mcp_tool_call/error)
  //        -> turn.completed(usage) | turn.failed(error)
  // --------------------------------------------------------------------------
  function codexItemName(item: Rec, fallback: string): string {
    const tool = str(item.tool);
    return tool ?? fallback;
  }

  function handleCodexLine(obj: Rec): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    const lineType = str(obj.type) ?? "";

    if (lineType === "thread.started") {
      setSession(str(obj.thread_id) ?? str(obj.threadId));
      out.push(statusEvent("running"));
      return out;
    }

    if (lineType === "turn.started") {
      return out;
    }

    if (lineType === "item.started" || lineType === "item.completed") {
      const item = asRec(obj.item);
      const itemType = str(item.type) ?? "";
      const itemId = str(item.id);

      if (lineType === "item.started") {
        if (itemType === "command_execution") {
          out.push(
            ev("tool_use", {
              toolUse: {
                name: "command_execution",
                id: itemId,
                input: str(item.command) ?? undefined,
              },
            })
          );
        } else if (itemType === "mcp_tool_call") {
          out.push(
            ev("tool_use", {
              toolUse: {
                name: codexItemName(item, itemType),
                id: itemId,
                input: inputString(item.arguments),
              },
            })
          );
        }
        return out;
      }

      // item.completed
      if (itemType === "agent_message") {
        const text = str(item.text);
        if (text !== undefined && text.length > 0) out.push(ev("text", { text }));
      } else if (itemType === "reasoning") {
        const summaryParts = arr(item.summary)
          .map((entry) => str(asRec(entry).text))
          .filter((t): t is string => t !== undefined && t.length > 0);
        const contentParts = arr(item.content)
          .map((entry) => str(asRec(entry).text))
          .filter((t): t is string => t !== undefined && t.length > 0);
        const thinking = summaryParts.length > 0 ? summaryParts.join("\n") : contentParts.join("\n");
        if (thinking.length > 0) out.push(ev("thinking", { thinking }));
      } else if (itemType === "command_execution") {
        const exitCode = num(item.exit_code);
        const isError =
          exitCode !== undefined ? exitCode !== 0 : (str(item.status) ?? "completed") === "failed";
        out.push(
          ev("tool_result", {
            toolResult: {
              name: "command_execution",
              toolUseId: itemId,
              output: resultOutput(item.aggregated_output),
              isError,
            },
          })
        );
      } else if (itemType === "mcp_tool_call") {
        const rawOutput = item.output ?? item.result;
        const isError = (str(item.status) ?? "completed") === "failed";
        out.push(
          ev("tool_result", {
            toolResult: {
              name: codexItemName(item, itemType),
              toolUseId: itemId,
              output: resultOutput(rawOutput),
              isError,
            },
          })
        );
      } else if (itemType === "error") {
        const message = str(item.message) ?? "codex item failed";
        out.push(errorEvent(message));
      }
      return out;
    }

    if (lineType === "turn.completed") {
      const usageRaw = asRec(obj.usage);
      const inputTokens = num(usageRaw.input_tokens) ?? 0;
      const outputTokens = num(usageRaw.output_tokens) ?? 0;
      out.push(
        usageEvent({
          inputTokens,
          outputTokens,
          totalTokens: inputTokens + outputTokens,
        })
      );
      out.push(statusEvent("done"));
      return out;
    }

    if (lineType === "turn.failed") {
      const message = str(asRec(obj.error).message) ?? "codex turn failed";
      out.push(errorEvent(message));
      out.push(statusEvent("interrupted"));
      return out;
    }

    if (lineType === "error") {
      const message = str(obj.message) ?? str(asRec(obj.error).message);
      if (message !== undefined) out.push(errorEvent(message));
      return out;
    }

    return out;
  }

  // --------------------------------------------------------------------------
  // opencode: step_start(sessionID) -> text/reasoning/tool parts ->
  //           step_finish(tokens/cost)
  // --------------------------------------------------------------------------
  function handleOpencodeLine(obj: Rec): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    setSession(str(obj.sessionID) ?? str(asRec(obj.part).sessionID));
    const lineType = (str(obj.type) ?? "").replace(/-/g, "_");

    if (lineType === "step_start") {
      out.push(statusEvent("running"));
      return out;
    }

    if (lineType === "text" || lineType === "reasoning") {
      const part = asRec(obj.part);
      const text = str(part.text);
      if (text !== undefined && text.length > 0) {
        out.push(lineType === "text" ? ev("text", { text }) : ev("thinking", { thinking: text }));
      }
      return out;
    }

    if (lineType === "tool") {
      const part = asRec(obj.part);
      const name = str(part.tool) ?? str(part.toolName) ?? "tool";
      const callId = str(part.callID) ?? str(part.callId) ?? str(part.id);
      const state = isRec(part.state) ? part.state : part;
      const stateStatus = str(state.status) ?? "completed";
      const input = inputString(state.input);
      out.push(ev("tool_use", { toolUse: { name, id: callId, input } }));
      if (stateStatus === "completed" || stateStatus === "error") {
        const isError = stateStatus === "error" || state.error !== undefined;
        out.push(
          ev("tool_result", {
            toolResult: {
              name,
              toolUseId: callId,
              output: isError
                ? (str(state.error) ?? resultOutput(state.output))
                : resultOutput(state.output),
              isError,
            },
          })
        );
      }
      return out;
    }

    if (lineType === "step_finish") {
      const part = asRec(obj.part);
      const tokens = asRec(part.tokens);
      const inputTokens = num(tokens.input) ?? 0;
      const outputTokens = num(tokens.output) ?? 0;
      const costRaw = part.cost;
      const costUsd =
        num(costRaw) ?? num(asRec(costRaw).total) ?? num(asRec(costRaw).usd);
      out.push(
        usageEvent({
          inputTokens,
          outputTokens,
          totalTokens: inputTokens + outputTokens,
          costUsd,
        })
      );
      out.push(statusEvent("done"));
      return out;
    }

    if (lineType === "error") {
      const message =
        str(obj.message) ??
        str(asRec(obj.error).message) ??
        str(asRec(asRec(obj.error).error).message);
      if (message !== undefined) {
        out.push(errorEvent(message));
        out.push(statusEvent("interrupted"));
      }
      return out;
    }

    return out;
  }

  // --------------------------------------------------------------------------
  // antigravity: init(conversation_id) -> step_update(text_delta, ~25ms
  //              tokens) -> result(response echo dedup + status)
  // --------------------------------------------------------------------------
  let agyAccumulated = "";

  function handleAntigravityLine(obj: Rec): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    setSession(str(obj.conversation_id) ?? str(obj.conversationId));
    const lineEvent = str(obj.event) ?? "";

    if (lineEvent === "init") {
      out.push(statusEvent("running"));
      return out;
    }

    if (lineEvent === "step_update") {
      const update = asRec(obj.step_update);
      const delta = str(update.text_delta);
      if (delta !== undefined && delta.length > 0) {
        const stepType = str(update.step_type) ?? "";
        if (stepType.includes("think")) {
          out.push(ev("thinking", { thinking: delta }));
        } else {
          agyAccumulated += delta;
          out.push(ev("text", { text: delta }));
        }
      }
      return out;
    }

    if (lineEvent === "result") {
      const result = asRec(obj.result);
      const response = str(result.response);
      // Duplicate suppression: `result.response` repeats the streamed deltas.
      // Emit only the uncovered tail; suppress entirely when already covered.
      if (response !== undefined && response.length > 0) {
        if (agyAccumulated.length === 0) {
          out.push(ev("text", { text: response }));
        } else if (
          response.startsWith(agyAccumulated) &&
          response.length > agyAccumulated.length
        ) {
          out.push(ev("text", { text: response.slice(agyAccumulated.length) }));
        }
        // exact match / suffix match / diverged -> suppress (conservative dedup)
      }
      agyAccumulated = "";
      const resultStatus = str(result.status) ?? "SUCCESS";
      if (resultStatus === "SUCCESS") {
        out.push(statusEvent("done"));
      } else {
        const message =
          str(asRec(result.error).message) ?? `antigravity result status ${resultStatus}`;
        out.push(errorEvent(message));
        out.push(statusEvent("interrupted"));
      }
      return out;
    }

    return out;
  }

  // --------------------------------------------------------------------------
  // cursor: system/init(session_id) -> user echo (SKIP) -> thinking deltas ->
  //         tool_call started/completed(<family>ToolCall, result.success|
  //         result.error.errorMessage) -> assistant blocks -> result
  // --------------------------------------------------------------------------
  const cursorSeenToolUseIds = new Set<string>();

  function cursorToolFamily(toolCall: Rec): { name: string; family: Rec } {
    for (const key of Object.keys(toolCall)) {
      if (key.endsWith("ToolCall")) {
        return { name: key.slice(0, -"ToolCall".length), family: asRec(toolCall[key]) };
      }
    }
    const firstKey = Object.keys(toolCall)[0] ?? "";
    return { name: firstKey || "tool", family: asRec(toolCall[firstKey]) };
  }

  function cursorToolResultPayload(
    callId: string | undefined,
    name: string,
    result: Rec
  ): OrchestratorEvent {
    if (result.error !== undefined) {
      const error = asRec(result.error);
      return ev("tool_result", {
        toolResult: {
          name,
          toolUseId: callId,
          output: str(error.errorMessage) ?? resultOutput(error),
          isError: true,
        },
      });
    }
    return ev("tool_result", {
      toolResult: {
        name,
        toolUseId: callId,
        output: resultOutput(result.success),
        isError: false,
      },
    });
  }

  function handleCursorLine(obj: Rec): OrchestratorEvent[] {
    const out: OrchestratorEvent[] = [];
    setSession(str(obj.session_id));
    const lineType = str(obj.type) ?? "";

    if (lineType === "system") {
      if ((str(obj.subtype) ?? "") === "init") out.push(statusEvent("running"));
      return out;
    }

    if (lineType === "user") {
      // Prompt echo — must be skipped, never rendered as assistant text.
      return out;
    }

    if (lineType === "thinking") {
      const text = str(obj.text) ?? str(asRec(obj.message).text);
      if (text !== undefined && text.length > 0) out.push(ev("thinking", { thinking: text }));
      return out;
    }

    if (lineType === "tool_call") {
      const callId = str(obj.call_id) ?? str(obj.callId);
      const { name, family } = cursorToolFamily(asRec(obj.tool_call));
      const subtype = str(obj.subtype) ?? "";
      if (subtype === "started") {
        cursorSeenToolUseIds.add(callId ?? name);
        out.push(
          ev("tool_use", {
            toolUse: { name, id: callId, input: inputString(family.args) },
          })
        );
      } else if (subtype === "completed") {
        if (callId === undefined || !cursorSeenToolUseIds.has(callId)) {
          if (callId !== undefined) cursorSeenToolUseIds.add(callId);
          out.push(
            ev("tool_use", {
              toolUse: { name, id: callId, input: inputString(family.args) },
            })
          );
        }
        out.push(cursorToolResultPayload(callId, name, asRec(family.result)));
      }
      return out;
    }

    if (lineType === "assistant") {
      const message = asRec(obj.message);
      for (const rawBlock of arr(message.content)) {
        const block = asRec(rawBlock);
        if (str(block.type) === "text") {
          const text = str(block.text);
          if (text !== undefined && text.length > 0) out.push(ev("text", { text }));
        }
      }
      return out;
    }

    if (lineType === "result") {
      const isError = obj.is_error === true || (str(obj.subtype) ?? "success") === "error";
      const usageRaw = asRec(obj.usage);
      const inputTokens = num(usageRaw.input_tokens) ?? 0;
      const outputTokens = num(usageRaw.output_tokens) ?? 0;
      const usage: TokenUsagePayload = {
        inputTokens,
        outputTokens,
        totalTokens: inputTokens + outputTokens,
        costUsd: num(obj.total_cost_usd) ?? num(obj.cost_usd),
      };
      if (isError) {
        const message =
          str(obj.result) ?? str(asRec(obj.error).message) ?? "cursor turn failed";
        out.push(errorEvent(message));
        out.push(usageEvent(usage));
        out.push(statusEvent("interrupted"));
      } else {
        out.push(usageEvent(usage));
        out.push(statusEvent("done"));
      }
      return out;
    }

    if (lineType === "error") {
      const message = str(obj.message) ?? str(asRec(obj.error).message);
      if (message !== undefined) out.push(errorEvent(message));
      return out;
    }

    return out;
  }

  // --------------------------------------------------------------------------
  // Dispatch & NDJSON framing
  // --------------------------------------------------------------------------

  function handleLine(line: string): OrchestratorEvent[] {
    const trimmed = line.trim();
    if (trimmed.length === 0) return [];
    if (!trimmed.startsWith("{")) return []; // banner/stdout noise: not protocol errors
    let obj: unknown;
    try {
      obj = JSON.parse(trimmed);
    } catch {
      return []; // torn / non-protocol JSON: skip rather than spam errors
    }
    if (!isRec(obj)) return [];

    switch (harness) {
      case "claude-code":
        return handleClaudeLine(obj);
      case "codex":
        return handleCodexLine(obj);
      case "opencode":
        return handleOpencodeLine(obj);
      case "antigravity":
        return handleAntigravityLine(obj);
      case "cursor":
        return handleCursorLine(obj);
      default:
        return [];
    }
  }

  return {
    pushLine(line: string): OrchestratorEvent[] {
      return handleLine(line);
    },
    pushChunk(chunk: string | Buffer): OrchestratorEvent[] {
      const text = typeof chunk === "string" ? chunk : chunk.toString("utf8");
      lineBuffer += text;
      const out: OrchestratorEvent[] = [];
      let newlineIndex = lineBuffer.indexOf("\n");
      while (newlineIndex !== -1) {
        const line = lineBuffer.slice(0, newlineIndex);
        lineBuffer = lineBuffer.slice(newlineIndex + 1);
        out.push(...handleLine(line));
        newlineIndex = lineBuffer.indexOf("\n");
      }
      return out;
    },
    flush(): OrchestratorEvent[] {
      const out: OrchestratorEvent[] = [];
      const remainder = lineBuffer;
      lineBuffer = "";
      if (remainder.trim().length > 0) out.push(...handleLine(remainder));
      if (harness === "claude-code") out.push(...flushClaude());
      return out;
    },
  };
}

// ============================================================================
// DriverStreamEvent -> OrchestratorEvent (Core SDK parsed events)
// ============================================================================

export interface NormalizeDriverEventOptions {
  /** Attached to the produced event when provided. */
  turnId?: string;
}

/**
 * Maps a Core SDK DriverStreamEvent to the OrchestratorEvent wire contract.
 * Returns null for events with no wire representation (raw lines).
 */
export function normalizeDriverStreamEvent(
  event: DriverStreamEvent,
  options: NormalizeDriverEventOptions = {}
): OrchestratorEvent | null {
  const turnId = options.turnId;
  const timestamp = new Date(event.timestamp).toISOString();
  const sessionId = event.harnessSessionId;
  const base: Partial<OrchestratorEvent> = {
    turnId,
    harness: event.harness,
    ...(sessionId === undefined ? {} : { sessionId }),
    timestamp,
  };

  switch (event.type) {
    case "thinking":
      return {
        ...base,
        kind: "thinking",
        thinking: event.thinking ?? event.text ?? "",
      } as OrchestratorEvent;
    case "text":
      return { ...base, kind: "text", text: event.text ?? "" } as OrchestratorEvent;
    case "tool_use":
      return {
        ...base,
        kind: "tool_use",
        toolUse: {
          name: event.toolName ?? "unknown",
          ...(event.toolId === undefined ? {} : { id: event.toolId }),
          ...(event.toolInput === undefined ? {} : { input: event.toolInput }),
        },
      } as OrchestratorEvent;
    case "tool_result":
      return {
        ...base,
        kind: "tool_result",
        toolResult: {
          ...(event.toolName === undefined ? {} : { name: event.toolName }),
          ...(event.toolId === undefined ? {} : { toolUseId: event.toolId }),
          output: event.toolOutput ?? "",
          ...(event.isError === undefined ? {} : { isError: event.isError }),
        },
      } as OrchestratorEvent;
    case "status":
      return {
        ...base,
        kind: "status",
        status: "running",
        ...(event.text === undefined ? {} : { text: event.text }),
      } as OrchestratorEvent;
    case "raw":
      return null;
    default:
      return null;
  }
}

// ============================================================================
// Transcript restoration: Core TranscriptMessage[] -> protocol 4-category view
// ============================================================================

/**
 * Restores a 4-category (user/assistant/thinking/tool) normalized history
 * from Core readHarnessTranscript output. Messages carrying multiple tool
 * invocations/results are split into one protocol message per item so the UI
 * renders exactly one toolUse/toolResult payload per message.
 */
export function normalizeTranscriptMessages(
  messages: TranscriptMessage[]
): NormalizedTranscriptMessage[] {
  const out: NormalizedTranscriptMessage[] = [];
  for (const message of messages) {
    const timestamp = message.timestamp === undefined ? {} : { timestamp: message.timestamp };
    const category: TranscriptCategory =
      message.category ??
      (message.toolUse !== undefined && message.toolUse.length > 0
        ? "tool_use"
        : message.toolResult !== undefined && message.toolResult.length > 0
          ? "tool_result"
          : message.role === "user"
            ? "user"
            : message.role === "system"
              ? "system"
              : "assistant");

    if (category === "tool_use") {
      const items = message.toolUse ?? [];
      if (items.length === 0) {
        out.push({ role: "tool", category, text: message.text, ...timestamp });
      } else {
        for (const item of items) {
          out.push({
            role: "tool",
            category,
            text: message.text,
            toolName: item.name,
            toolUse: {
              name: item.name,
              ...(item.id === undefined ? {} : { id: item.id }),
              ...(item.input === undefined ? {} : { input: item.input }),
            },
            ...timestamp,
          });
        }
      }
      continue;
    }

    if (category === "tool_result") {
      const items = message.toolResult ?? [];
      if (items.length === 0) {
        out.push({ role: "tool", category, text: message.text, ...timestamp });
      } else {
        for (const item of items) {
          out.push({
            role: "tool",
            category,
            text: item.output,
            toolName: item.name,
            toolResult: {
              ...(item.name === undefined ? {} : { name: item.name }),
              ...(item.toolUseId === undefined ? {} : { toolUseId: item.toolUseId }),
              output: item.output,
              ...(item.isError === undefined ? {} : { isError: item.isError }),
            },
            ...timestamp,
          });
        }
      }
      continue;
    }

    if (category === "thinking") {
      const thinking = message.thinking ?? message.text;
      out.push({
        role: "assistant",
        category,
        text: message.text,
        ...(thinking === undefined || thinking.length === 0 ? {} : { thinking }),
        ...timestamp,
      });
      continue;
    }

    // user / assistant / system: base message, then split any attached tool
    // items (mergeToolResults mode merges results onto the assistant message).
    const role: NormalizedTranscriptMessage["role"] =
      category === "user" ? "user" : category === "system" ? "system" : "assistant";
    out.push({
      role,
      category,
      text: message.text,
      ...(message.thinking === undefined || message.thinking.length === 0
        ? {}
        : { thinking: message.thinking }),
      ...timestamp,
    });
    for (const item of message.toolUse ?? []) {
      out.push({
        role: "tool",
        category: "tool_use",
        text: "",
        toolName: item.name,
        toolUse: {
          name: item.name,
          ...(item.id === undefined ? {} : { id: item.id }),
          ...(item.input === undefined ? {} : { input: item.input }),
        },
        ...timestamp,
      });
    }
    for (const item of message.toolResult ?? []) {
      out.push({
        role: "tool",
        category: "tool_result",
        text: item.output,
        toolName: item.name,
        toolResult: {
          ...(item.name === undefined ? {} : { name: item.name }),
          ...(item.toolUseId === undefined ? {} : { toolUseId: item.toolUseId }),
          output: item.output,
          ...(item.isError === undefined ? {} : { isError: item.isError }),
        },
        ...timestamp,
      });
    }
  }
  return out;
}
