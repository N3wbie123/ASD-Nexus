import React, { useEffect, useRef } from "react";
import { MessageBlock, type ChatTimelineItem } from "./MessageBlock.tsx";

interface ChatViewProps {
  items: ChatTimelineItem[];
  isLoadingTranscript: boolean;
  isStreaming: boolean;
  onQuickPrompt?: (text: string) => void;
}

const STARTER_PROMPTS = [
  "Summarize this repository structure and key modules.",
  "Reply with exactly: M1_READY",
  "Check git status and list recent commits.",
];

export const ChatView: React.FC<ChatViewProps> = ({
  items,
  isLoadingTranscript,
  isStreaming,
  onQuickPrompt,
}) => {
  const bottomRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [items.length, items[items.length - 1]?.text, items[items.length - 1]?.thinking]);

  if (isLoadingTranscript) {
    return (
      <div className="chat-empty-state">
        <span className="spinner spinner-lg" />
        <p>Restoring Claude transcript...</p>
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="chat-empty-state">
        <div className="empty-hero-badge">ASD v2.6.0 · Claude Code M1</div>
        <h2>Start a streamed Claude session</h2>
        <p>
          Messages run through the ASD orchestrator server via SSE (`runDriverTurn`)
          and automatically preserve native Claude `--resume` continuity across turns.
        </p>
        {onQuickPrompt ? (
          <div className="starter-grid">
            {STARTER_PROMPTS.map((prompt) => (
              <button
                key={prompt}
                type="button"
                className="starter-chip"
                onClick={() => onQuickPrompt(prompt)}
              >
                {prompt}
              </button>
            ))}
          </div>
        ) : null}
      </div>
    );
  }

  return (
    <div className="chat-timeline" data-testid="chat-timeline">
      {items.map((item) => (
        <MessageBlock key={item.id} item={item} />
      ))}
      {isStreaming ? (
        <div className="streaming-indicator" data-testid="streaming-indicator">
          <span className="spinner" />
          <span>Claude is streaming...</span>
        </div>
      ) : null}
      <div ref={bottomRef} />
    </div>
  );
};
