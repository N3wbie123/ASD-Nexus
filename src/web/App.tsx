import React, { useCallback, useEffect, useRef, useState } from "react";
import type {
  DiscoveredSessionItem,
  GetBoardResponse,
  OrchestratorEvent,
} from "../shared/events.ts";
import {
  fetchBoard,
  fetchHealth,
  fetchSessions,
  fetchTranscript,
  promoteSession,
  streamTurn,
} from "./api/sse-client.ts";
import { ChatView } from "./components/ChatView.tsx";
import type { ChatTimelineItem } from "./components/MessageBlock.tsx";
import { PromptInput } from "./components/PromptInput.tsx";
import {
  getSessionItemKey,
  SessionSidebar,
} from "./components/SessionSidebar.tsx";

let nextIdSeq = 0;
function uid(prefix: string): string {
  nextIdSeq += 1;
  return `${prefix}-${Date.now()}-${nextIdSeq}`;
}

export const App: React.FC = () => {
  const [sdkVersion, setSdkVersion] = useState<string>("2.6.0");
  const [projectRoot, setProjectRoot] = useState<string>("");
  const [sessions, setSessions] = useState<DiscoveredSessionItem[]>([]);
  const [selectedSession, setSelectedSession] =
    useState<DiscoveredSessionItem | null>(null);
  const [activeAsdSessionId, setActiveAsdSessionId] = useState<string | null>(
    null
  );
  const [activeHarnessSessionId, setActiveHarnessSessionId] = useState<
    string | null
  >(null);
  const [model, setModel] = useState<string>("");
  const [timeline, setTimeline] = useState<ChatTimelineItem[]>([]);
  const [board, setBoard] = useState<GetBoardResponse | null>(null);
  const [isLoadingSessions, setIsLoadingSessions] = useState(false);
  const [isLoadingTranscript, setIsLoadingTranscript] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [statusError, setStatusError] = useState<string | null>(null);

  const abortRef = useRef<AbortController | null>(null);

  const loadWorkspace = useCallback(async (root: string) => {
    if (!root.trim()) return;
    setIsLoadingSessions(true);
    setStatusError(null);
    try {
      const [sessRes, boardRes] = await Promise.all([
        fetchSessions(root, "claude-code"),
        fetchBoard(root).catch(() => null),
      ]);
      setSessions(sessRes.sessions);
      if (boardRes) setBoard(boardRes);
    } catch (err) {
      setStatusError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoadingSessions(false);
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    fetchHealth(controller.signal)
      .then((h) => {
        setSdkVersion(h.version);
        setProjectRoot(h.defaultProjectRoot);
        return loadWorkspace(h.defaultProjectRoot);
      })
      .catch((err) => {
        if ((err as Error).name !== "AbortError") {
          setStatusError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => controller.abort();
  }, [loadWorkspace]);

  const handleNewChat = useCallback(() => {
    abortRef.current?.abort();
    setSelectedSession(null);
    setActiveAsdSessionId(null);
    setActiveHarnessSessionId(null);
    setTimeline([]);
    setStatusError(null);
  }, []);

  const handleSelectSession = useCallback(
    async (sess: DiscoveredSessionItem) => {
      abortRef.current?.abort();
      setSelectedSession(sess);
      setActiveAsdSessionId(sess.asdSessionId ?? null);
      setActiveHarnessSessionId(sess.harnessSessionId || null);
      if (sess.model) setModel(sess.model);
      setStatusError(null);

      if (!sess.harnessSessionId) {
        setTimeline([]);
        return;
      }

      setIsLoadingTranscript(true);
      try {
        const res = await fetchTranscript(
          sess.harness,
          sess.harnessSessionId,
          projectRoot,
          sess.transcriptPath
        );
        const mapped: ChatTimelineItem[] = res.messages.map((m, idx) => {
          const cat = m.category ?? (m.role === "user" ? "user" : "assistant");
          if (cat === "user") {
            return { id: `hist-${idx}`, kind: "user", text: m.text, timestamp: m.timestamp };
          }
          if (cat === "thinking") {
            return {
              id: `hist-${idx}`,
              kind: "thinking",
              thinking: m.thinking ?? m.text,
              timestamp: m.timestamp,
            };
          }
          if (cat === "tool_use") {
            return {
              id: `hist-${idx}`,
              kind: "tool_use",
              toolUse: m.toolUse ?? { name: m.toolName ?? "tool", input: m.text },
              isCompletedTool: true,
              timestamp: m.timestamp,
            };
          }
          if (cat === "tool_result") {
            return {
              id: `hist-${idx}`,
              kind: "tool_result",
              toolResult: m.toolResult ?? { name: m.toolName, output: m.text },
              timestamp: m.timestamp,
            };
          }
          return {
            id: `hist-${idx}`,
            kind: "text",
            text: m.text,
            timestamp: m.timestamp,
          };
        });
        setTimeline(mapped);
      } catch (err) {
        setStatusError(err instanceof Error ? err.message : String(err));
      } finally {
        setIsLoadingTranscript(false);
      }
    },
    [projectRoot]
  );

  const applySseEvent = useCallback((ev: OrchestratorEvent) => {
    if (ev.sessionId) {
      setActiveAsdSessionId(ev.sessionId);
    }
    const rawObj =
      ev.raw && typeof ev.raw === "object"
        ? (ev.raw as { harnessSessionId?: string })
        : null;
    if (rawObj?.harnessSessionId) {
      setActiveHarnessSessionId(rawObj.harnessSessionId);
    }

    setTimeline((prev) => {
      const next = [...prev];

      if (ev.kind === "thinking" && ev.thinking) {
        const last = next[next.length - 1];
        if (last && last.kind === "thinking" && last.isStreaming) {
          next[next.length - 1] = {
            ...last,
            thinking: (last.thinking ?? "") + ev.thinking,
          };
        } else {
          next.push({
            id: uid("think"),
            kind: "thinking",
            thinking: ev.thinking,
            isStreaming: true,
            timestamp: ev.timestamp,
          });
        }
        return next;
      }

      if (ev.kind === "tool_use" && ev.toolUse) {
        // Close any active thinking block
        for (let i = next.length - 1; i >= 0; i -= 1) {
          if (next[i]?.isStreaming && next[i]?.kind === "thinking") {
            next[i] = { ...next[i]!, isStreaming: false };
          }
        }
        next.push({
          id: uid("tuse"),
          kind: "tool_use",
          toolUse: ev.toolUse,
          isStreaming: true,
          isCompletedTool: false,
          timestamp: ev.timestamp,
        });
        return next;
      }

      if (ev.kind === "tool_result" && ev.toolResult) {
        const matchId = ev.toolResult.toolUseId;
        for (let i = next.length - 1; i >= 0; i -= 1) {
          const item = next[i];
          if (
            item?.kind === "tool_use" &&
            !item.isCompletedTool &&
            (!matchId || item.toolUse?.id === matchId)
          ) {
            next[i] = { ...item, isStreaming: false, isCompletedTool: true };
            break;
          }
        }
        next.push({
          id: uid("tres"),
          kind: "tool_result",
          toolResult: ev.toolResult,
          timestamp: ev.timestamp,
        });
        return next;
      }

      if (ev.kind === "text" && ev.text) {
        for (let i = next.length - 1; i >= 0; i -= 1) {
          if (next[i]?.isStreaming && next[i]?.kind === "thinking") {
            next[i] = { ...next[i]!, isStreaming: false };
          }
        }
        const last = next[next.length - 1];
        if (last && last.kind === "text" && last.isStreaming) {
          const prevText = last.text ?? "";
          const sep =
            prevText && !prevText.endsWith("\n") && !ev.text.startsWith("\n")
              ? "\n"
              : "";
          next[next.length - 1] = {
            ...last,
            text: prevText + sep + ev.text,
          };
        } else {
          next.push({
            id: uid("txt"),
            kind: "text",
            text: ev.text,
            isStreaming: true,
            timestamp: ev.timestamp,
          });
        }
        return next;
      }

      if (ev.kind === "usage" && ev.usage) {
        next.push({
          id: uid("use"),
          kind: "usage",
          usage: ev.usage,
          timestamp: ev.timestamp,
        });
        return next;
      }

      if (ev.kind === "error" && ev.error) {
        next.push({
          id: uid("err"),
          kind: "error",
          error: ev.error,
          timestamp: ev.timestamp,
        });
        return next;
      }

      if (
        ev.kind === "status" &&
        (ev.status === "done" || ev.status === "interrupted")
      ) {
        return next.map((item) =>
          item.isStreaming
            ? {
                ...item,
                isStreaming: false,
                ...(item.kind === "tool_use" ? { isCompletedTool: true } : {}),
              }
            : item
        );
      }

      return next;
    });
  }, []);

  const handleSendPrompt = useCallback(
    async (prompt: string) => {
      if (isStreaming || !projectRoot.trim()) return;
      setStatusError(null);
      setIsStreaming(true);

      setTimeline((prev) => [
        ...prev,
        {
          id: uid("usr"),
          kind: "user",
          text: prompt,
          timestamp: new Date().toISOString(),
        },
      ]);

      const controller = new AbortController();
      abortRef.current = controller;

      try {
        let sessionIdToUse = activeAsdSessionId ?? undefined;

        // If user selected a plain CLI session, promote it first so resume continues seamlessly
        if (!sessionIdToUse && selectedSession?.harnessSessionId) {
          const promoted = await promoteSession({
            projectRoot,
            harness: selectedSession.harness,
            harnessSessionId: selectedSession.harnessSessionId,
            model: model.trim() || selectedSession.model,
            transcriptPath: selectedSession.transcriptPath,
            makeForeground: false,
            source: "gui",
          });
          sessionIdToUse = promoted.sessionId;
          setActiveAsdSessionId(promoted.sessionId);
        }

        await streamTurn(
          {
            projectRoot,
            sessionId: sessionIdToUse,
            harness: "claude-code",
            ...(model.trim() ? { model: model.trim() } : {}),
            prompt,
            source: "gui",
          },
          applySseEvent,
          controller.signal
        );
      } catch (err) {
        if ((err as Error).name !== "AbortError") {
          const message = err instanceof Error ? err.message : String(err);
          setTimeline((prev) => [
            ...prev.map((i) => ({ ...i, isStreaming: false })),
            { id: uid("err"), kind: "error", error: message },
          ]);
        }
      } finally {
        setIsStreaming(false);
        abortRef.current = null;
        void loadWorkspace(projectRoot);
      }
    },
    [
      activeAsdSessionId,
      applySseEvent,
      isStreaming,
      loadWorkspace,
      model,
      projectRoot,
      selectedSession,
    ]
  );

  const selectedKey = selectedSession
    ? getSessionItemKey(selectedSession)
    : activeAsdSessionId
      ? `asd:${activeAsdSessionId}`
      : null;

  return (
    <div className="app-shell">
      <SessionSidebar
        projectRoot={projectRoot}
        onProjectRootChange={setProjectRoot}
        sessions={sessions}
        selectedKey={selectedKey}
        isLoading={isLoadingSessions}
        onRefresh={() => void loadWorkspace(projectRoot)}
        onNewChat={handleNewChat}
        onSelectSession={(s) => void handleSelectSession(s)}
      />

      <main className="main-pane">
        <header className="top-bar">
          <div className="top-bar-meta">
            <span className="pill">SDK v{sdkVersion}</span>
            <span className="pill" data-testid="asd-session-pill">
              ASD: {activeAsdSessionId ?? "new"}
            </span>
            <span className="pill" data-testid="claude-session-pill">
              Claude Resume:{" "}
              {activeHarnessSessionId
                ? activeHarnessSessionId.slice(0, 12)
                : "none"}
            </span>
          </div>
          <div className="top-bar-board">
            {board?.sessionId ? (
              <span className="pill pill-board">
                Board {board.progress ?? "0/0"} ({board.phase ?? "idle"})
              </span>
            ) : null}
          </div>
        </header>

        {statusError ? (
          <div className="top-error-strip">{statusError}</div>
        ) : null}

        <ChatView
          items={timeline}
          isLoadingTranscript={isLoadingTranscript}
          isStreaming={isStreaming}
          onQuickPrompt={(p) => void handleSendPrompt(p)}
        />

        <PromptInput
          disabled={!projectRoot.trim()}
          isStreaming={isStreaming}
          model={model}
          onModelChange={setModel}
          onSubmit={(p) => void handleSendPrompt(p)}
          onAbort={() => abortRef.current?.abort()}
        />
      </main>
    </div>
  );
};
