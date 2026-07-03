/**
 * BottomPanel — View-owned collapsible bottom panel.
 *
 * Tabs:
 *   Terminal — honest placeholder (PTY-over-WS is a separate backend workstream)
 *   Output   — live tool-call activity log (timestamp, tool name, one-line result)
 *   Debug    — raw WS frames for this session (ring buffer, last 200), filter box
 *
 * Output + Debug are pure-UI off existing WS frames (message.appended with
 * toolCalls). No backend changes.
 *
 * The frame ring buffer lives in the parent (BottomPanel) so it captures
 * frames even when the Debug tab isn't the active tab.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../components/Icon";
import { useMessages } from "../../../hooks/queries";
import { onFrame } from "../../../api";
import type { Message, ServerFrame, ToolCall } from "../../../types";
import { fmtTime } from "../helpers";

export type BpTab = "terminal" | "output" | "debug";

// ── Types ──────────────────────────────────────────────

interface OutputEntry {
  ts: number;
  toolName: string;
  label: string | null;
  argSummary: string;
  result: string | null;
}

interface DebugEntry {
  ts: number;
  frame: ServerFrame;
}

const MAX_DEBUG = 200;

/** Truncate a string to one line + N chars for the output log. */
function oneline(s: string, max = 120): string {
  const firstLine = s.split("\n")[0] ?? "";
  return firstLine.length > max ? firstLine.slice(0, max - 1) + "…" : firstLine;
}

/** Extract a short human label from a tool call's args. */
function toolArgSummary(tc: ToolCall): string {
  const args = tc.args;
  if (!args) return "";
  if (typeof args === "string") return args.slice(0, 60);
  if (typeof args === "object") {
    const obj = args as Record<string, unknown>;
    // Common arg keys that make good summaries
    const key =
      "command" in obj ? "command" :
      "query" in obj ? "query" :
      "path" in obj ? "path" :
      "pattern" in obj ? "pattern" :
      "code" in obj ? "code" :
      "expr" in obj ? "expr" :
      "goal" in obj ? "goal" :
      null;
    if (key && typeof obj[key] === "string") {
      return String(obj[key]).slice(0, 60);
    }
  }
  return "";
}

/** Extract tool calls from any message that carries them. */
function extractToolCalls(m: Message): ToolCall[] {
  if (!m.toolCalls || m.toolCalls.length === 0) return [];
  return m.toolCalls;
}

function safeStringify(obj: unknown): string {
  try {
    return JSON.stringify(obj, null, 2);
  } catch {
    return String(obj);
  }
}

// ── Component ──────────────────────────────────────────

export function BottomPanel({
  sessionId,
  height,
  tab,
  onTabChange,
  onClose,
}: {
  sessionId: string | null;
  height?: number;
  tab: BpTab;
  onTabChange: (t: BpTab) => void;
  onClose: () => void;
}) {
  const tabs: Array<{ id: BpTab; label: string }> = [
    { id: "terminal", label: "Terminal" },
    { id: "output", label: "Output" },
    { id: "debug", label: "Debug" },
  ];

  // ── Frame ring buffer (always active, regardless of active tab) ──
  const [debugFrames, setDebugFrames] = useState<DebugEntry[]>([]);

  // Reset on session change.
  useEffect(() => {
    setDebugFrames([]);
  }, [sessionId]);

  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      // Only capture frames relevant to this session, or global frames when
      // no session is selected.
      const sid = sessionId;
      if (sid) {
        const frameSid = (frame as { sessionId?: string }).sessionId;
        if (frameSid && frameSid !== sid) return;
      }
      const entry: DebugEntry = { ts: Date.now() / 1000, frame };
      setDebugFrames((prev) => {
        const next = [...prev, entry];
        if (next.length > MAX_DEBUG) next.shift();
        return next;
      });
    });
    return unsub;
  }, [sessionId]);

  const handleClearFrames = useCallback(() => {
    setDebugFrames([]);
  }, []);

  return (
    <div className="bpanel" style={height ? { height } : undefined}>
      <div className="bp-tabs">
        <div className="bp-tablist">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`bp-tab${tab === t.id ? " on" : ""}`}
              onClick={() => onTabChange(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="bp-right">
          <button type="button" className="icobtn" title="Close panel" onClick={onClose}>
            <Icon name="chevron-down" size={13} />
          </button>
        </div>
      </div>
      <div className="bp-body">
        {tab === "terminal" && <TerminalTab />}
        {tab === "output" && <OutputTab sessionId={sessionId} />}
        {tab === "debug" && (
          <DebugTab
            frames={debugFrames}
            onClear={handleClearFrames}
          />
        )}
      </div>
    </div>
  );
}

// ── Terminal tab — honest placeholder ──────────────────

function TerminalTab() {
  return (
    <div className="bp-placeholder">
      <Icon name="terminal" size={20} />
      <span className="d">Terminal attach requires envoy PTY support (planned).</span>
    </div>
  );
}

// ── Output tab — tool-call activity log ────────────────

function OutputTab({ sessionId }: { sessionId: string | null }) {
  const { data: msgData } = useMessages(sessionId);
  const messages = msgData?.messages ?? [];

  // Seed entries from loaded messages (tool calls on assistant/tool messages).
  // These already have results (loaded from the server after completion).
  const seeded = useMemo(() => {
    const entries: OutputEntry[] = [];
    for (const m of messages) {
      const calls = extractToolCalls(m);
      for (const tc of calls) {
        entries.push({
          ts: m.timestamp,
          toolName: tc.name,
          label: tc.label ?? null,
          argSummary: toolArgSummary(tc),
          result: tc.result ?? null,
        });
      }
    }
    return entries;
  }, [messages]);

  // Live entries appended from message.appended WS frames.
  const [liveEntries, setLiveEntries] = useState<OutputEntry[]>([]);
  const [liveSince, setLiveSince] = useState(0);

  // Reset live state when session changes.
  useEffect(() => {
    setLiveEntries([]);
    setLiveSince(Date.now() / 1000);
  }, [sessionId]);

  // Listen for message.appended frames carrying tool calls for this session.
  useEffect(() => {
    if (!sessionId) return;
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind !== "message.appended") return;
      if (frame.sessionId !== sessionId) return;
      const msg = frame.message;
      const calls = extractToolCalls(msg);
      if (calls.length === 0) return;
      const now = msg.timestamp;
      setLiveEntries((prev) => [
        ...prev,
        ...calls.map((tc) => ({
          ts: now,
          toolName: tc.name,
          label: tc.label ?? null,
          argSummary: toolArgSummary(tc),
          result: tc.result ?? null,
        })),
      ]);
    });
    return unsub;
  }, [sessionId]);

  // Merge: seeded entries whose ts predates liveSince + live entries.
  // Avoids duplicates if the server re-sends an older message via appended.
  const entries = useMemo(() => {
    const base = seeded.filter((e) => e.ts < liveSince);
    return [...base, ...liveEntries];
  }, [seeded, liveEntries, liveSince]);

  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [entries.length]);

  if (entries.length === 0) {
    return (
      <div className="bp-placeholder">
        <Icon name="activity" size={20} />
        <span className="d">No tool activity yet for this session.</span>
      </div>
    );
  }

  return (
    <div className="bp-output" ref={scrollRef}>
      {entries.map((e, i) => (
        <div className="ln" key={i}>
          <span className="ts">{fmtTime(e.ts)}</span>
          <span className="tn">{e.label ?? e.toolName}</span>
          {e.argSummary && <span className="ar d">{oneline(e.argSummary, 80)}</span>}
          {e.result != null ? (
            <span className="rs g">{oneline(e.result)}</span>
          ) : (
            <span className="rs a">running…</span>
          )}
        </div>
      ))}
    </div>
  );
}

// ── Debug tab — raw WS frame ring buffer + filter ──────

function DebugTab({
  frames,
  onClear,
}: {
  frames: DebugEntry[];
  onClear: () => void;
}) {
  const [filter, setFilter] = useState("");
  const [paused, setPaused] = useState(false);
  const [displayFrames, setDisplayFrames] = useState<DebugEntry[]>(frames);

  // When paused, freeze the displayed frames; when resumed, sync.
  useEffect(() => {
    if (!paused) setDisplayFrames(frames);
  }, [frames, paused]);

  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scrollRef.current && !paused) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [displayFrames.length, paused]);

  const filtered = useMemo(() => {
    if (!filter.trim()) return displayFrames;
    const q = filter.toLowerCase();
    return displayFrames.filter((e) => {
      try {
        return JSON.stringify(e.frame).toLowerCase().includes(q);
      } catch {
        return false;
      }
    });
  }, [displayFrames, filter]);

  return (
    <div className="bp-debug">
      <div className="bp-debug-bar">
        <Icon name="search" size={12} />
        <input
          className="bp-filter"
          type="text"
          placeholder="Filter frames…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button
          type="button"
          className={`bp-mini-btn${paused ? " on" : ""}`}
          onClick={() => setPaused((v) => !v)}
          title={paused ? "Resume capture" : "Pause capture"}
        >
          {paused ? "Resume" : "Pause"}
        </button>
        <button
          type="button"
          className="bp-mini-btn"
          onClick={onClear}
          title="Clear buffer"
        >
          <Icon name="trash" size={12} />
        </button>
        <span className="bp-count d">{filtered.length}/{displayFrames.length}</span>
      </div>
      <div className="bp-debug-list" ref={scrollRef}>
        {filtered.length === 0 ? (
          <div className="bp-placeholder">
            <span className="d">
              {displayFrames.length === 0
                ? "Waiting for WebSocket frames…"
                : "No frames match the filter."}
            </span>
          </div>
        ) : (
          filtered.map((e, i) => (
            <div className="ln bp-frame" key={i}>
              <span className="ts">{fmtTime(e.ts)}</span>
              <span className="fk">{e.frame.kind}</span>
              <span className="fj">{safeStringify(e.frame)}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
