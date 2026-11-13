import React from "react";
import type { DiscoveredSessionItem } from "../../shared/events.ts";

interface SessionSidebarProps {
  projectRoot: string;
  onProjectRootChange: (nextRoot: string) => void;
  sessions: DiscoveredSessionItem[];
  selectedKey: string | null;
  isLoading: boolean;
  onRefresh: () => void;
  onNewChat: () => void;
  onSelectSession: (session: DiscoveredSessionItem) => void;
}

export function getSessionItemKey(s: DiscoveredSessionItem): string {
  return s.asdSessionId
    ? `asd:${s.asdSessionId}`
    : `cli:${s.harness}:${s.harnessSessionId}`;
}

function formatRelativeTime(epochMs: number): string {
  if (!epochMs) return "";
  const diffSec = Math.max(0, Math.floor((Date.now() - epochMs) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return new Date(epochMs).toLocaleDateString();
}

export const SessionSidebar: React.FC<SessionSidebarProps> = ({
  projectRoot,
  onProjectRootChange,
  sessions,
  selectedKey,
  isLoading,
  onRefresh,
  onNewChat,
  onSelectSession,
}) => {
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <div className="brand-title">ASD-Nexus</div>
        <span className="brand-pill">M1 · Claude</span>
      </div>

      <div className="sidebar-project">
        <label htmlFor="project-root-input" className="field-label">
          Project Root
        </label>
        <div className="project-row">
          <input
            id="project-root-input"
            className="text-input"
            type="text"
            value={projectRoot}
            onChange={(e) => onProjectRootChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onRefresh();
            }}
            placeholder="C:\path\to\project"
          />
          <button
            type="button"
            className="btn-secondary"
            onClick={onRefresh}
            disabled={isLoading}
            title="Discover sessions"
          >
            {isLoading ? "..." : "Sync"}
          </button>
        </div>
      </div>

      <button
        type="button"
        className="btn-primary new-chat-btn"
        onClick={onNewChat}
        data-testid="new-chat-btn"
      >
        + New Claude Chat
      </button>

      <div className="session-list-header">
        <span>Claude Sessions ({sessions.length})</span>
      </div>

      <div className="session-list" data-testid="session-list">
        {sessions.length === 0 ? (
          <div className="session-empty">
            No Claude sessions discovered in this project yet.
          </div>
        ) : (
          sessions.map((s) => {
            const key = getSessionItemKey(s);
            const active = key === selectedKey;
            const label =
              s.summary?.trim() ||
              (s.harnessSessionId
                ? `Claude ${s.harnessSessionId.slice(0, 8)}`
                : s.asdSessionId ?? "New Session");
            return (
              <button
                key={key}
                type="button"
                className={`session-item ${active ? "session-item-active" : ""}`}
                onClick={() => onSelectSession(s)}
              >
                <div className="session-item-top">
                  <span
                    className={`badge ${
                      s.isPromotedToAsd ? "badge-asd" : "badge-cli"
                    }`}
                  >
                    {s.isPromotedToAsd ? "ASD" : "CLI"}
                  </span>
                  <span className="session-time">
                    {formatRelativeTime(s.updatedAt)}
                  </span>
                </div>
                <div className="session-title">{label}</div>
                <div className="session-ids">
                  {s.model ? <span>{s.model}</span> : null}
                  {s.harnessSessionId ? (
                    <code>{s.harnessSessionId.slice(0, 10)}</code>
                  ) : (
                    <code>pending-id</code>
                  )}
                </div>
              </button>
            );
          })
        )}
      </div>
    </aside>
  );
};
