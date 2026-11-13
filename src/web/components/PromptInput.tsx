import React, { useState } from "react";

interface PromptInputProps {
  disabled?: boolean;
  isStreaming: boolean;
  model: string;
  onModelChange: (model: string) => void;
  onSubmit: (prompt: string) => void;
  onAbort: () => void;
}

export const PromptInput: React.FC<PromptInputProps> = ({
  disabled,
  isStreaming,
  model,
  onModelChange,
  onSubmit,
  onAbort,
}) => {
  const [draft, setDraft] = useState("");

  const handleSend = (): void => {
    const trimmed = draft.trim();
    if (!trimmed || isStreaming || disabled) return;
    onSubmit(trimmed);
    setDraft("");
  };

  return (
    <div className="composer">
      <div className="composer-controls">
        <label htmlFor="claude-model-select" className="composer-label">
          Claude Model
        </label>
        <input
          id="claude-model-select"
          className="model-input"
          type="text"
          value={model}
          onChange={(e) => onModelChange(e.target.value)}
          placeholder="default (or e.g. claude-sonnet-4-6)"
          disabled={isStreaming}
        />
        <span className="composer-hint">Enter to send · Shift+Enter for newline</span>
      </div>
      <div className="composer-box">
        <textarea
          data-testid="prompt-textarea"
          className="prompt-textarea"
          rows={3}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              handleSend();
            }
          }}
          placeholder="Ask Claude Code anything..."
          disabled={disabled}
        />
        {isStreaming ? (
          <button
            type="button"
            className="btn-danger send-btn"
            onClick={onAbort}
            data-testid="stop-btn"
          >
            Stop
          </button>
        ) : (
          <button
            type="button"
            className="btn-primary send-btn"
            onClick={handleSend}
            disabled={disabled || !draft.trim()}
            data-testid="send-btn"
          >
            Send
          </button>
        )}
      </div>
    </div>
  );
};
