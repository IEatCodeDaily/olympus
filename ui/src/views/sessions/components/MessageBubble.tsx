/**
 * MessageBubble — a single message in the transcript.
 *
 * Design:
 *  - User = right-aligned elevated bubble; agent = left/default.
 *  - Steer = user bubble with dashed border + ⚡ badge (pending → delivered).
 *  - Assistant tool calls are interleaved at their `anchor` offset inside
 *    the markdown body — not dumped at the bottom. Each card carries its
 *    lifecycle status (pending / in_progress / completed / failed).
 *  - Toolbar at bottom: [Copy] [Fork] <datetime> (hover-only).
 */

import React, { useState, useCallback, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Icon } from "../../../components/Icon";
import type { Message, ToolCall } from "../../../types";
import { fmtDateTime, isDiffResult } from "../helpers";
import { ToolCard } from "./ToolCard";
import { DiffCard } from "./DiffCard";

export const MessageBubble = React.memo(function MessageBubble({
  msg,
  steerPending = false,
  onFork,
}: {
  msg: Message;
  steerPending?: boolean;
  onFork: () => void;
}) {
  const isUser = msg.role === "user";
  const isSteer = msg.role === "user" && msg.finishReason === "steer";
  const isSystem = msg.role === "system" || msg.role === "session_meta";
  const dt = fmtDateTime(msg.timestamp);
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
      <div className="msg-system" data-ts={dt}>
        <span className="gk">{msg.content}</span>
      </div>
    );
  }

  if (msg.role === "tool") return null;

  // Build chronologically interleaved segments: text split at each tool call's
  // anchor offset, with the card inserted between segments.
  const segments = useMemo(() => {
    const text = msg.content ?? "";
    const calls = msg.toolCalls ?? [];
    if (calls.length === 0) return [{ type: "text" as const, text }];

    // Calls without an anchor go to the end (legacy / old persisted messages).
    const anchored = calls.filter((c) => c.anchor != null);
    const unanchored = calls.filter((c) => c.anchor == null);

    // Sort anchored calls by their offset within the text.
    const sorted = [...anchored].sort((a, b) => (a.anchor! - b.anchor!));

    const segs: Array<{ type: "text"; text: string } | { type: "call"; tc: ToolCall }> = [];
    let cursor = 0;
    let cardIdx = 0;
    for (const tc of sorted) {
      const off = tc.anchor!;
      // Clamp to text length (anchors are codepoint offsets; slice works on
      // UTF-16 code units, but for typical ASCII-heavy markdown this is fine;
      // worst case the split point is slightly off but no content is lost).
      const safeOff = Math.min(off, text.length);
      if (safeOff > cursor) {
        segs.push({ type: "text", text: text.slice(cursor, safeOff) });
      }
      segs.push({ type: "call", tc });
      cardIdx++;
      cursor = safeOff;
    }
    if (cursor < text.length) {
      segs.push({ type: "text", text: text.slice(cursor) });
    }
    // Append unanchored calls at the end.
    for (const tc of unanchored) {
      segs.push({ type: "call", tc });
    }
    return segs;
  }, [msg.content, msg.toolCalls]);

  return (
    <div className={isSteer ? "msg-user msg-steer" : isUser ? "msg-user" : "msg-ai"} data-ts={dt}>
      {isSteer && (
        <span className={`steer-badge ${steerPending ? "steer-pending" : "steer-delivered"}`}>
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
          </svg>
          {steerPending ? "steer · waiting" : "steer · delivered"}
        </span>
      )}

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

      {/* Content — interleaved for assistant, plain for user */}
      {isUser ? (
        <span>{msg.content}</span>
      ) : (
        segments.map((seg, i) => {
          if (seg.type === "call") {
            return isDiffResult(seg.tc) ? (
              <DiffCard key={`tc-${i}`} tc={seg.tc} />
            ) : (
              <ToolCard
                key={`tc-${i}`}
                tc={seg.tc}
                idx={i}
                expanded={tcExpanded.has(i)}
                onToggle={toggleTc}
              />
            );
          }
          return (
            <ReactMarkdown key={`t-${i}`} remarkPlugins={[remarkGfm]}>
              {seg.text}
            </ReactMarkdown>
          );
        })
      )}

      {/* Toolbar: [Copy] [Fork] datetime */}
      <div className="msg-toolbar">
        <button type="button" className="mt-btn" onClick={handleCopy} title="Copy">
          <Icon name={copied ? "check" : "copy"} size={12} />
        </button>
        <button type="button" className="mt-btn" title="Fork from here" onClick={onFork}>
          <Icon name="git-branch" size={12} />
        </button>
        <span className="mt-dt">{dt}</span>
      </div>
    </div>
  );
});

export type { ToolCall };
