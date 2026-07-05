/**
 * MessageBubble — a single message in the transcript.
 *
 * Bug fixes applied:
 *  - Bug 9: No "ASSISTANT" / "TOOL" text header. Styling conveys role.
 *    User = right-aligned elevated bubble; agent = left/default.
 *  - Bug 8: Tool calls render as collapsible dropdowns
 *    (icon + toolName + status), expandable to args + output.
 */

import React, { useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Icon } from "../../../components/Icon";
import type { Message, ToolCall } from "../../../types";
import { fmtTime, isDiffResult } from "../helpers";
import { ToolCard } from "./ToolCard";
import { DiffCard } from "./DiffCard";

export const MessageBubble = React.memo(function MessageBubble({
  msg,
  onFork,
}: {
  msg: Message;
  onFork: () => void;
}) {
  const isUser = msg.role === "user";
  const isSteer = msg.role === "user" && msg.finishReason === "steer";
  const isSystem = msg.role === "system" || msg.role === "session_meta";
  const ts = fmtTime(msg.timestamp);
  const [copied, setCopied] = useState(false);
  const [tcExpanded, setTcExpanded] = useState<Set<number>>(new Set());
  const [reasonExpanded, setReasonExpanded] = useState(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard?.writeText(msg.content ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [msg.content]);

  const toggleTc = useCallback((idx: number) => {
    setTcExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }, []);

  if (isSystem) {
    return (
      <div className="msg-system" data-ts={ts}>
        <span className="gk">{msg.content}</span>
      </div>
    );
  }

  // Tool-result messages (role === "tool") are SUPPRESSED — they are the
  // result of a tool call already shown inline in the assistant message's
  // toolCalls array. Rendering them separately creates duplicate dropdowns.
  if (msg.role === "tool") {
    return null;
  }

  return (
    <div className={isSteer ? "msg-user msg-steer" : isUser ? "msg-user" : "msg-ai"} data-ts={ts}>
      {isSteer && (
        <span className="steer-badge">
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
          </svg>
          steer
        </span>
      )}
      {/* Bug 9: removed the .who ASSISTANT/TOOL header line entirely. */}

      {/* Reasoning block */}
      {msg.reasoning && (
        <div className="reasoning-block">
          <button
            type="button"
            className="reasoning-toggle"
            onClick={() => setReasonExpanded((v) => !v)}
          >
            <Icon name={reasonExpanded ? "chevron-down" : "chevron-right"} size={11} />
            <span className="gk" style={{ fontSize: 10 }}>
              thinking
            </span>
          </button>
          {reasonExpanded && (
            <div className="reasoning-body">{msg.reasoning}</div>
          )}
        </div>
      )}

      {/* Content */}
      {isUser ? (
        <span>{msg.content}</span>
      ) : (
        <ReactMarkdown remarkPlugins={[remarkGfm]}>
          {msg.content ?? ""}
        </ReactMarkdown>
      )}

      {/* Tool calls embedded in assistant message */}
      {msg.toolCalls && msg.toolCalls.length > 0 && (
        <div className="tc-list">
          {msg.toolCalls.map((tc, idx) =>
            isDiffResult(tc) ? (
              <DiffCard key={idx} tc={tc} />
            ) : (
              <ToolCard
                key={idx}
                tc={tc}
                idx={idx}
                expanded={tcExpanded.has(idx)}
                onToggle={toggleTc}
              />
            ),
          )}
        </div>
      )}

      {/* Message actions */}
      <div className="msg-acts">
        <button type="button" onClick={handleCopy} title="Copy">
          <Icon name={copied ? "check" : "copy"} size={12} />
        </button>
        <button
          type="button"
          title="Fork from here"
          onClick={onFork}
        >
          <Icon name="git-branch" size={12} />
        </button>
        <span className="ts">{ts}</span>
      </div>
    </div>
  );
});

// Re-export for convenience
export type { ToolCall };
