import type {
  GetBoardResponse,
  GetSessionsResponse,
  GetTranscriptResponse,
  OrchestratorEvent,
  PromoteSessionRequest,
  PromoteSessionResponse,
  TurnStreamRequest,
} from "../../shared/events.ts";

export interface HealthResponse {
  ok: boolean;
  version: string;
  milestone: string;
  defaultProjectRoot: string;
}

async function readJsonOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) {
    let message = `HTTP ${res.status}`;
    try {
      const errBody = (await res.json()) as { error?: string };
      if (errBody?.error) message = errBody.error;
    } catch {
      // ignore non-json error bodies
    }
    throw new Error(message);
  }
  return (await res.json()) as T;
}

export async function fetchHealth(signal?: AbortSignal): Promise<HealthResponse> {
  const res = await fetch("/api/health", { signal });
  return readJsonOrThrow<HealthResponse>(res);
}

export async function fetchSessions(
  projectRoot: string,
  harness = "claude-code",
  signal?: AbortSignal
): Promise<GetSessionsResponse> {
  const params = new URLSearchParams({ projectRoot, harness, limit: "100" });
  const res = await fetch(`/api/sessions?${params.toString()}`, { signal });
  return readJsonOrThrow<GetSessionsResponse>(res);
}

export async function promoteSession(
  body: PromoteSessionRequest
): Promise<PromoteSessionResponse> {
  const res = await fetch("/api/sessions/promote", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return readJsonOrThrow<PromoteSessionResponse>(res);
}

export async function fetchTranscript(
  harness: string,
  harnessSessionId: string,
  projectRoot: string,
  transcriptPath?: string,
  signal?: AbortSignal
): Promise<GetTranscriptResponse> {
  const params = new URLSearchParams({ projectRoot, limit: "300" });
  if (transcriptPath) params.set("transcriptPath", transcriptPath);
  const res = await fetch(
    `/api/transcript/${encodeURIComponent(harness)}/${encodeURIComponent(harnessSessionId)}?${params.toString()}`,
    { signal }
  );
  return readJsonOrThrow<GetTranscriptResponse>(res);
}

export async function fetchBoard(
  projectRoot: string,
  signal?: AbortSignal
): Promise<GetBoardResponse> {
  const params = new URLSearchParams({ projectRoot });
  const res = await fetch(`/api/board?${params.toString()}`, { signal });
  return readJsonOrThrow<GetBoardResponse>(res);
}

/**
 * Streams a turn via `POST /api/turn/stream` and invokes `onEvent` for every
 * normalized SSE `OrchestratorEvent` frame, handling split UTF-8/frame chunks.
 */
export async function streamTurn(
  request: TurnStreamRequest,
  onEvent: (event: OrchestratorEvent) => void,
  signal?: AbortSignal
): Promise<void> {
  const res = await fetch("/api/turn/stream", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream",
    },
    body: JSON.stringify(request),
    signal,
  });

  if (!res.ok) {
    let message = `HTTP ${res.status}`;
    try {
      const errBody = (await res.json()) as { error?: string };
      if (errBody?.error) message = errBody.error;
    } catch {
      // ignore
    }
    throw new Error(message);
  }

  if (!res.body) {
    throw new Error("Streaming response body unavailable");
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  const processFrame = (frame: string): void => {
    const lines = frame.split(/\r?\n/);
    for (const line of lines) {
      if (!line.startsWith("data:")) continue;
      const payload = line.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;
      try {
        const parsed = JSON.parse(payload) as OrchestratorEvent;
        onEvent(parsed);
      } catch {
        // Ignore malformed frame lines
      }
    }
  };

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    let sepMatch = /\r?\n\r?\n/.exec(buffer);
    while (sepMatch) {
      const idx = sepMatch.index;
      const frame = buffer.slice(0, idx);
      buffer = buffer.slice(idx + sepMatch[0].length);
      if (frame.trim()) processFrame(frame);
      sepMatch = /\r?\n\r?\n/.exec(buffer);
    }
  }

  const tail = (buffer + decoder.decode()).trim();
  if (tail) processFrame(tail);
}
