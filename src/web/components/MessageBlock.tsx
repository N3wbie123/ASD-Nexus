import React from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type {
  TokenUsagePayload,
  ToolResultPayload,
  ToolUsePayload,
} from "../../shared/events.ts";

export interface ChatTimelineItem {
  id: string;
  kind:
    | "user"
    | "thinking"
    | "tool_use"
    | "tool_result"
    | "text"
    | "usage"
    | "error"
    | "status";
  text?: string;
  thinking?: string;
  toolUse?: ToolUsePayload;
  toolResult?: ToolResultPayload;
  usage?: TokenUsagePayload;
  status?: "running" | "done" | "interrupted";
  error?: string;
  timestamp?: string;
  isStreaming?: boolean;
  isCompletedTool?: boolean;
}

interface MessageBlockProps {
  item: ChatTimelineItem;
}

function formatJsonPretty(raw?: string): string {
  if (!raw) return "";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

export const MessageBlock: React.FC<MessageBlockProps> = ({ item }) => {
  if (item.kind === "user") {
    return (
      <div className="msg-row msg-row-user" data-testid="msg-user">
        <div className="msg-bubble msg-bubble-user">
          <div className="msg-role-tag">You</div>
          <div className="msg-plain-text">{item.text}</div>
        </div>
      </div>
    );
  }

  if (item.kind === "thinking") {
    return (
      <details
        className="msg-card msg-thinking"
        open={Boolean(item.isStreaming)}
        data-testid="msg-thinking"
      >
        <summary className="msg-card-summary">
          <span className="badge badge-thinking">Thinking</span>
          <span className="summary-hint">
            {item.isStreaming ? "Reasoning live..." : "Inspect reasoning"}
          </span>
        </summary>
        <pre className="msg-pre">{item.thinking ?? item.text ?? ""}</pre>
      </details>
    );
  }

  if (item.kind === "tool_use") {
    const active = Boolean(item.isStreaming && !item.isCompletedTool);
    return (
      <details className="msg-card msg-tool-use" data-testid="msg-tool-use">
        <summary className="msg-card-summary">
          <span className="tool-icon" aria-hidden="true">
            {active ? <span className="spinner" /> : "⚙"}
          </span>
          <span className="badge badge-tool">{item.toolUse?.name ?? "tool"}</span>
          {item.toolUse?.id ? (
            <code className="mono-chip">{item.toolUse.id.slice(0, 12)}</code>
          ) : null}
          <span className="summary-hint">
            {active ? "Executing..." : "Arguments"}
          </span>
        </summary>
        {item.toolUse?.input ? (
          <pre className="msg-pre">{formatJsonPretty(item.toolUse.input)}</pre>
        ) : (
          <div className="msg-empty-note">(no arguments)</div>
        )}
      </details>
    );
  }

  if (item.kind === "tool_result") {
    const isError = Boolean(item.toolResult?.isError);
    return (
      <details
        className={`msg-card msg-tool-result ${isError ? "msg-tool-result-error" : ""}`}
        open={isError}
        data-testid="msg-tool-result"
      >
        <summary className="msg-card-summary">
          <span className={`badge ${isError ? "badge-error" : "badge-result"}`}>
            {isError ? "Tool Error" : "Tool Result"}
          </span>
          {item.toolResult?.name ? (
            <code className="mono-chip">{item.toolResult.name}</code>
          ) : null}
          <span className="summary-hint">
            {isError ? "Failed" : "View output"}
          </span>
        </summary>
        <pre className="msg-pre">{item.toolResult?.output ?? item.text ?? ""}</pre>
      </details>
    );
  }

  if (item.kind === "text") {
    return (
      <div className="msg-row msg-row-assistant" data-testid="msg-assistant">
        <div className="msg-bubble msg-bubble-assistant">
          <div className="msg-role-tag">Claude</div>
          <div className="msg-markdown">
            <Markdown remarkPlugins={[remarkGfm]}>{item.text ?? ""}</Markdown>
          </div>
        </div>
      </div>
    );
  }

  if (item.kind === "usage") {
    const u = item.usage;
    return (
      <div className="msg-usage-footer" data-testid="msg-usage">
        <span>
          Tokens: {u?.inputTokens ?? 0} in / {u?.outputTokens ?? 0} out
        </span>
        {u?.costUsd !== undefined ? (
          <span>Cost: ${u.costUsd.toFixed(5)}</span>
        ) : null}
      </div>
    );
  }

  if (item.kind === "error") {
    return (
      <div className="msg-error-banner" role="alert" data-testid="msg-error">
        <strong>Error:</strong> {item.error ?? item.text ?? "Unknown error"}
      </div>
    );
  }

  return null;
};
