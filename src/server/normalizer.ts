/**
 * Event & Transcript Normalizer (src/server/normalizer.ts)
 *
 * Normalizes Core SDK `DriverStreamEvent` / raw NDJSON lines and
 * `TranscriptMessage` records into the shared M1 `OrchestratorEvent` and
 * `NormalizedTranscriptMessage` contracts.
 */

import type {
  DriverStreamEvent,
  TranscriptMessage,
  TranscriptMessageCategory,
} from "./sdk.ts";
import type {
  NormalizedTranscriptMessage,
  OrchestratorEvent,
  TokenUsagePayload,
} from "../shared/events.ts";

export interface TurnNormalizerOptions {
  sessionId: string;
  turnId: string;
}

export interface TurnEventNormalizer {
  onOutputChunk(chunk: string): OrchestratorEvent[];
  fromDriverEvent(event: DriverStreamEvent): OrchestratorEvent[];
  getHarnessSessionId(): string | null;
}

function isoNow(ms?: number): string {
  return new Date(ms ?? Date.now()).toISOString();
}

function toFiniteNumber(val: unknown): number | undefined {
  return typeof val === "number" && Number.isFinite(val) ? val : undefined;
}

function extractUsageFromResultObj(obj: Record<string, unknown>): TokenUsagePayload | null {
  const usageObj =
    obj.usage && typeof obj.usage === "object"
      ? (obj.usage as Record<string, unknown>)
      : null;

  const inputTokens = toFiniteNumber(usageObj?.input_tokens);
  const outputTokens = toFiniteNumber(usageObj?.output_tokens);
  const cacheCreate = toFiniteNumber(usageObj?.cache_creation_input_tokens) ?? 0;
  const cacheRead = toFiniteNumber(usageObj?.cache_read_input_tokens) ?? 0;
  const costUsd =
    toFiniteNumber(obj.total_cost_usd) ?? toFiniteNumber(obj.cost_usd);

  const computedInput =
    inputTokens !== undefined ? inputTokens + cacheCreate + cacheRead : undefined;
  const totalTokens =
    computedInput !== undefined || outputTokens !== undefined
      ? (computedInput ?? 0) + (outputTokens ?? 0)
      : undefined;

  if (
    computedInput === undefined &&
    outputTokens === undefined &&
    costUsd === undefined
  ) {
    return null;
  }

  return {
    ...(computedInput !== undefined ? { inputTokens: computedInput } : {}),
    ...(outputTokens !== undefined ? { outputTokens } : {}),
    ...(totalTokens !== undefined ? { totalTokens } : {}),
    ...(costUsd !== undefined ? { costUsd } : {}),
  };
}

/**
 * Stateful per-turn normalizer that:
 * - Maps Core `DriverStreamEvent` objects into `OrchestratorEvent`.
 * - Extracts token usage & error flags from final `result` JSON lines.
 * - Deduplicates Claude's trailing `result` text when identical assistant text was already streamed.
 */
export function createTurnEventNormalizer(
  options: TurnNormalizerOptions
): TurnEventNormalizer {
  const { sessionId, turnId } = options;
  let lineBuffer = "";
  let streamedText = "";
  let lastResultText = "";
  let detectedHarnessSessionId: string | null = null;
  let usageEmitted = false;

  function makeBase(timestamp?: number): {
    sessionId: string;
    turnId: string;
    timestamp: string;
  } {
    return {
      sessionId,
      turnId,
      timestamp: isoNow(timestamp),
    };
  }

  function processOutputLine(line: string): OrchestratorEvent[] {
    const trimmed = line.trim();
    if (!trimmed.startsWith("{")) return [];

    let obj: Record<string, unknown>;
    try {
      const parsed = JSON.parse(trimmed);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return [];
      }
      obj = parsed as Record<string, unknown>;
    } catch {
      return [];
    }

    const events: OrchestratorEvent[] = [];

    if (typeof obj.session_id === "string" && obj.session_id.trim()) {
      const nextSid = obj.session_id.trim();
      if (nextSid !== detectedHarnessSessionId) {
        detectedHarnessSessionId = nextSid;
        events.push({
          ...makeBase(),
          kind: "status",
          status: "running",
          raw: { harnessSessionId: nextSid },
        });
      }
    }

    if (obj.type === "result") {
      if (typeof obj.result === "string") {
        lastResultText = obj.result.trim();
      }
      if (obj.is_error === true) {
        const errMsg =
          typeof obj.result === "string" && obj.result.trim()
            ? obj.result.trim()
            : typeof obj.error === "string" && obj.error.trim()
              ? obj.error.trim()
              : "Harness turn returned an error";
        events.push({
          ...makeBase(),
          kind: "error",
          error: errMsg,
        });
      }
      if (!usageEmitted) {
        const usage = extractUsageFromResultObj(obj);
        if (usage) {
          usageEmitted = true;
          events.push({
            ...makeBase(),
            kind: "usage",
            usage,
          });
        }
      }
    }

    return events;
  }

  return {
    onOutputChunk(chunk: string): OrchestratorEvent[] {
      lineBuffer += chunk;
      const out: OrchestratorEvent[] = [];
      let nlIdx = lineBuffer.indexOf("\n");
      while (nlIdx !== -1) {
        const line = lineBuffer.slice(0, nlIdx).replace(/\r$/, "");
        lineBuffer = lineBuffer.slice(nlIdx + 1);
        out.push(...processOutputLine(line));
        nlIdx = lineBuffer.indexOf("\n");
      }
      return out;
    },

    fromDriverEvent(event: DriverStreamEvent): OrchestratorEvent[] {
      const out: OrchestratorEvent[] = [];
      const base = makeBase(event.timestamp);

      if (event.harnessSessionId && event.harnessSessionId !== detectedHarnessSessionId) {
        detectedHarnessSessionId = event.harnessSessionId;
        if (event.type !== "status") {
          out.push({
            ...base,
            kind: "status",
            status: "running",
            raw: { harnessSessionId: event.harnessSessionId },
          });
        }
      }

      switch (event.type) {
        case "thinking":
          if (event.thinking) {
            out.push({
              ...base,
              kind: "thinking",
              thinking: event.thinking,
            });
          }
          break;

        case "tool_use":
          out.push({
            ...base,
            kind: "tool_use",
            toolUse: {
              name: event.toolName ?? "tool",
              ...(event.toolId ? { id: event.toolId } : {}),
              ...(event.toolInput !== undefined ? { input: event.toolInput } : {}),
            },
          });
          break;

        case "tool_result":
          out.push({
            ...base,
            kind: "tool_result",
            toolResult: {
              ...(event.toolName ? { name: event.toolName } : {}),
              ...(event.toolId ? { toolUseId: event.toolId } : {}),
              output: event.toolOutput ?? "",
              ...(event.isError ? { isError: true } : {}),
            },
          });
          break;

        case "text": {
          const text = event.text ?? "";
          if (!text) break;
          const trimmedText = text.trim();
          const isTrailingResultEcho =
            Boolean(event.harnessSessionId) &&
            streamedText.trim().length > 0 &&
            (trimmedText === streamedText.trim() ||
              (lastResultText.length > 0 &&
                trimmedText === lastResultText &&
                streamedText.trim().endsWith(lastResultText)));
          if (!isTrailingResultEcho) {
            streamedText += (streamedText && !streamedText.endsWith("\n") && !text.startsWith("\n") ? "\n" : "") + text;
            out.push({
              ...base,
              kind: "text",
              text,
            });
          }
          break;
        }

        case "status":
          out.push({
            ...base,
            kind: "status",
            status: "running",
            ...(event.harnessSessionId
              ? { raw: { harnessSessionId: event.harnessSessionId } }
              : {}),
          });
          break;

        case "raw": {
          const rawText = event.text?.trim();
          if (rawText) {
            const looksLikeError =
              event.isError === true ||
              /^(error|fatal|exception|authentication|unauthorized)\b/i.test(rawText);
            out.push(
              looksLikeError
                ? { ...base, kind: "error", error: rawText }
                : { ...base, kind: "status", status: "running", raw: { text: rawText } }
            );
          }
          break;
        }
      }

      return out;
    },

    getHarnessSessionId(): string | null {
      return detectedHarnessSessionId;
    },
  };
}

function normalizeRole(role: string): "user" | "assistant" | "system" | "tool" {
  switch (role) {
    case "user":
    case "assistant":
    case "system":
    case "tool":
      return role;
    default:
      return "assistant";
  }
}

function inferCategory(msg: TranscriptMessage): TranscriptMessageCategory {
  if (msg.category) return msg.category;
  if (msg.role === "user") return "user";
  if (msg.role === "system") return "system";
  if (msg.thinking) return "thinking";
  if (msg.toolUse && msg.toolUse.length > 0) return "tool_use";
  if (msg.toolResult && msg.toolResult.length > 0) return "tool_result";
  return "assistant";
}

/**
 * Expands Core `TranscriptMessage` entries (which may combine thinking, toolUse[],
 * toolResult[], and text on one turn record) into flat `NormalizedTranscriptMessage[]`
 * items aligned with the 4-category web chat renderer.
 */
export function normalizeTranscriptMessages(
  messages: TranscriptMessage[]
): NormalizedTranscriptMessage[] {
  const out: NormalizedTranscriptMessage[] = [];

  for (const msg of messages) {
    const baseRole = normalizeRole(msg.role);
    const hasStructuredParts =
      Boolean(msg.thinking) ||
      Boolean(msg.toolUse?.length) ||
      Boolean(msg.toolResult?.length);

    if (!hasStructuredParts) {
      out.push({
        role: baseRole,
        category: inferCategory(msg),
        text: msg.text ?? "",
        ...(msg.timestamp ? { timestamp: msg.timestamp } : {}),
      });
      continue;
    }

    if (msg.thinking && msg.thinking.trim()) {
      out.push({
        role: "assistant",
        category: "thinking",
        text: msg.thinking,
        thinking: msg.thinking,
        ...(msg.timestamp ? { timestamp: msg.timestamp } : {}),
      });
    }

    if (msg.toolUse) {
      for (const tu of msg.toolUse) {
        out.push({
          role: "assistant",
          category: "tool_use",
          text: tu.input ?? "",
          toolName: tu.name,
          toolUse: {
            name: tu.name,
            ...(tu.id ? { id: tu.id } : {}),
            ...(tu.input !== undefined ? { input: tu.input } : {}),
          },
          ...(msg.timestamp ? { timestamp: msg.timestamp } : {}),
        });
      }
    }

    if (msg.toolResult) {
      for (const tr of msg.toolResult) {
        out.push({
          role: "tool",
          category: "tool_result",
          text: tr.output ?? "",
          ...(tr.name ? { toolName: tr.name } : {}),
          toolResult: {
            ...(tr.name ? { name: tr.name } : {}),
            ...(tr.toolUseId ? { toolUseId: tr.toolUseId } : {}),
            output: tr.output ?? "",
            ...(tr.isError ? { isError: true } : {}),
          },
          ...(msg.timestamp ? { timestamp: msg.timestamp } : {}),
        });
      }
    }

    if (msg.text && msg.text.trim()) {
      out.push({
        role: baseRole === "tool" ? "assistant" : baseRole,
        category: baseRole === "user" ? "user" : baseRole === "system" ? "system" : "assistant",
        text: msg.text,
        ...(msg.timestamp ? { timestamp: msg.timestamp } : {}),
      });
    }
  }

  return out;
}
