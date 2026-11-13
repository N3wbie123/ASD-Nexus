import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import { normalizeTranscriptMessages } from "../normalizer.ts";
import type { GetTranscriptResponse } from "../../shared/protocol.ts";

export function getTranscriptRoute(
  ctx: RequestContext,
  harness: string,
  harnessSessionId: string
): void {
  const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
  const transcriptPath = ctx.query.get("transcriptPath") || undefined;
  const maxMessagesStr = ctx.query.get("maxMessages") || ctx.query.get("limit");
  const maxMessages = maxMessagesStr ? Number(maxMessagesStr) : undefined;
  const beforeStr = ctx.query.get("before");
  const before = beforeStr ? Number(beforeStr) : undefined;

  try {
    const raw = ctx.deps.readHarnessTranscript(harness as any, harnessSessionId, {
      transcriptPath,
      limit: maxMessages,
      before,
      homeDir: ctx.homeDir,
    });

    if (raw.error && !raw.found) {
      const resp: GetTranscriptResponse = {
        harness,
        harnessSessionId,
        transcriptPath: raw.transcriptPath,
        messages: [],
        error: raw.error,
      };
      sendJson(ctx.res, 200, resp);
      return;
    }

    const normalized = normalizeTranscriptMessages(raw.messages || []);
    const resp: GetTranscriptResponse = {
      harness,
      harnessSessionId,
      transcriptPath: raw.transcriptPath,
      messages: normalized,
      hasMore: raw.hasMore,
      truncated: raw.truncated,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    const resp: GetTranscriptResponse = {
      harness,
      harnessSessionId,
      messages: [],
      error: err instanceof Error ? err.message : String(err),
    };
    sendJson(ctx.res, 200, resp);
  }
}
