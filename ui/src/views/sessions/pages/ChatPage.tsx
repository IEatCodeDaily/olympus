/**
 * ChatPage — viewport content for the active session chat.
 *
 * Owns the transcript + composer ONLY (Page content). The View owns
 * the surrounding layout (right sidebar, bottom panel, header).
 *
 * Live-turn UX:
 *  - Optimistic message append; "thinking…" via /ws (idle → thinking →
 *    streaming → done).
 *  - REHYDRATION: on mount/refresh, if the session's server-derived liveness
 *    is "running", the thinking state is restored (the drain loop is still
 *    running server-side; deltas resume over WS).
 *  - QUEUE: messages typed while a turn runs are queued in cards above the
 *    composer (reorder/edit/delete). Each card has a Steer button to inject
 *    it into the running turn immediately. When the turn finishes, the queue
 *    auto-drains head-first as the next prompt.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Icon } from "../../../components/Icon";
import { useSession, useMessages, qk } from "../../../hooks/queries";
import {
  sendMessage,
  cancelSession,
  steerSession,
  forkSession,
  onFrame,
  respondPermission,
  sendFrame,
  getDisplayName,
} from "../../../api";
import type { Message, ServerFrame, ToolCall } from "../../../types";
import { MessageBubble } from "../components/MessageBubble";
import { ToolCard } from "../components/ToolCard";
import { DiffCard } from "../components/DiffCard";
import { isDiffResult } from "../helpers";
import { Composer } from "../components/Composer";
import { ForkModal } from "../components/ForkModal";
import { QueuePanel, type QueuedMsg } from "../components/QueuePanel";

type AgentStatus = "idle" | "thinking" | "streaming" | "done";

/** A chunk of the in-flight assistant turn, in arrival order. */
type StreamPart =
  | { type: "text"; text: string }
  | { type: "toolCall"; toolCall: ToolCall }
  | { type: "reasoning"; text: string };

// ── Per-session queue persistence (survives refresh & session switches) ──
function queueKey(sessionId: string): string {
  return `olympus-queue-${sessionId}`;
}

function loadQueue(sessionId: string): QueuedMsg[] {
  try {
    const raw = localStorage.getItem(queueKey(sessionId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as QueuedMsg[];
    return Array.isArray(parsed) ? parsed.filter((m) => m?.id && m?.text) : [];
  } catch {
    return [];
  }
}

function saveQueue(sessionId: string, items: QueuedMsg[]) {
  try {
    if (items.length === 0) localStorage.removeItem(queueKey(sessionId));
    else localStorage.setItem(queueKey(sessionId), JSON.stringify(items));
  } catch {
    // storage full/unavailable — queue lives in memory only
  }
}

export function ChatPage({
  sessionId,
  onForkRequested,
}: {
  sessionId: string;
  onForkRequested?: (sessionId: string) => void;
}) {
  const { data: session } = useSession(sessionId);
  const { data: msgData, isLoading } = useMessages(sessionId);
  const navigate = useNavigate();
  const qc = useQueryClient();

  // streaming + status — ALL session-scoped, reset on session change.
  // streamParts preserves arrival order so text, tool calls, and reasoning
  // interleave naturally as the agent works.
  const [streamParts, setStreamParts] = useState<StreamPart[]>([]);
  const streamingText = streamParts
    .filter((p) => p.type === "text")
    .map((p) => p.text)
    .join("");
  const [sending, setSending] = useState(false);
  const [agentStatus, setAgentStatus] = useState<AgentStatus>("idle");
  const [text, setText] = useState("");
  const [optimisticMsg, setOptimisticMsg] = useState<Message | null>(null);
  const [queue, setQueue] = useState<QueuedMsg[]>([]);
  // Steer message IDs still waiting to be processed by the agent.
  // The backend broadcasts a `steer.delivered` session.log when the
  // steer-ack Done is consumed; IDs leave this set at that point.
  const [pendingSteers, setPendingSteers] = useState<Set<number>>(new Set());
  // Pending permission request (ACP session/request_permission) for this session.
  const [permission, setPermission] = useState<{
    toolCall: string;
    options: Array<{ optionId: string; name: string; kind: string }>;
  } | null>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);
  // The model/thinking used for the last send — reused when the queue drains.
  const lastSendOpts = useRef<{ model?: string; thinking?: string }>({});

  // ── S8: session-scoped WS subscription + typing presence ────────────
  // Who is typing in THIS session right now (sessionId, who) → expiresAt.
  // Filtered to the current session on receive; TTL-expired client-side.
  const [typers, setTypers] = useState<Map<string, number>>(new Map());
  // Debounce timer ref for outbound typing frames (~3 s).
  const typingDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Our own display name (so we don't show our own typing indicator).
  const myName = useMemo(() => getDisplayName(), []);

  // ── Reset ALL transient state when switching sessions (Bug: thinking leak) ──
  // Without this, agentStatus/streamingText/sending from the previous session
  // persist across the route transition because React Router reuses the same
  // component instance for /sessions/$sessionId → /sessions/$otherId.
  useEffect(() => {
    setStreamParts([]);
    setSending(false);
    setAgentStatus("idle");
    setOptimisticMsg(null);
    setPermission(null);
    setText("");
    setQueue(loadQueue(sessionId));
    setTypers(new Map());
  }, [sessionId]);

  // Persist queue on every change.
  useEffect(() => {
    saveQueue(sessionId, queue);
  }, [sessionId, queue]);

  // ── S8: Subscribe to this session's message stream on mount ──────────
  // The server defaults to firehose, so we explicitly narrow to this session
  // to avoid flooding every open tab with every session's deltas. We
  // unsubscribe on unmount / session switch. (Backward compatible: the
  // server still delivers session-list-level frames regardless.)
  useEffect(() => {
    sendFrame({ kind: "subscribe", sessionIds: [sessionId] });
    // ADR 0020 v2 §4.2 — deliver-on-(re)subscribe. Navigating away drops this
    // session's frames (should_deliver); on return we force a refetch of the
    // durable transcript so a turn completed while we were gone is reconstructed
    // (with durable-first done, the assistant row is guaranteed committed).
    void qc.invalidateQueries({ queryKey: qk.messages(sessionId) });
    // The same reconstruction is needed when the WS itself drops and comes
    // back: frames broadcast during the outage are gone (no replay), so a
    // completed turn would otherwise never appear until manual navigation
    // away and back. Resubscribe (the new socket has no subscription state)
    // and refetch the durable transcript + session liveness.
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "ws.reconnected") {
        sendFrame({ kind: "subscribe", sessionIds: [sessionId] });
        void qc.invalidateQueries({ queryKey: qk.messages(sessionId) });
        void qc.invalidateQueries({ queryKey: qk.session(sessionId) });
      }
    });
    return () => {
      unsub();
      sendFrame({ kind: "unsubscribe", sessionIds: [sessionId] });
    };
  }, [sessionId, qc]);

  // ── S8: TTL sweep for expired typers ──────────────────────────────────
  // A cheap interval that prunes any typer whose expiresAt has passed.
  useEffect(() => {
    const timer = setInterval(() => {
      setTypers((prev) => {
        if (prev.size === 0) return prev;
        const now = Date.now() / 1000;
        let changed = false;
        const next = new Map();
        for (const [who, expiresAt] of prev) {
          if (expiresAt > now) {
            next.set(who, expiresAt);
          } else {
            changed = true;
          }
        }
        return changed ? next : prev;
      });
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  // ── Rehydrate thinking state on refresh/mount ─────────────────────
  // The server's liveness is authoritative: "running" means the drain loop is
  // mid-turn server-side. On browser refresh the local `sending` state is
  // lost but the turn is still going — restore the indicator so the user
  // isn't staring at a silent chat. Deltas resume over WS automatically.
  // Symmetric: a refetched "idle" clears stale local thinking/streaming — the
  // WS `session.updated {liveness: idle}` frame that normally clears it is
  // exactly what gets dropped when the socket was down or unsubscribed, and
  // without this the spinner/stream tail is stuck until manual navigation.
  // Safe against races: the server marks in-flight before any delta flows, so
  // a refetch issued after a send observes "running"; a refetched "idle"
  // genuinely means the turn is over. (Worst case is a brief flicker when a
  // pre-send refetch resolves late; the next session.updated frame restores
  // thinking — self-healing, preferable to a permanently stuck indicator.)
  useEffect(() => {
    if (session?.liveness === "running" && !sending) {
      setSending(true);
      setAgentStatus("thinking");
    }
    if (session?.liveness === "idle" && sending) {
      setSending(false);
      setAgentStatus("idle");
      // Also drop any stale stream tail: if message.done was lost, the
      // partial stream would render below the (refetched) committed
      // assistant row as duplicated content.
      setStreamParts([]);
    }
    // Only react to liveness — deliberately not `sending` (would re-trigger).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.liveness, sessionId]);

  const serverMessages = msgData?.messages ?? [];
  const hasServerEcho = serverMessages.some(
    (m) => m.role === "user" && m.content === optimisticMsg?.content && m.messageId >= 0,
  );
  // ADR 0020 v2 §4.3 — render strictly by the existing per-session `message_id`
  // (server truth), with the optimistic user bubble (messageId -1) always last.
  // This is what prevents the reported reorder: a durable assistant row (higher
  // message_id) can never sort above a newer, not-yet-committed user message.
  const messages = (
    optimisticMsg && !hasServerEcho ? [...serverMessages, optimisticMsg] : serverMessages
  )
    .slice()
    .sort((a, b) => {
      const ai = a.messageId < 0 ? Number.MAX_SAFE_INTEGER : a.messageId;
      const bi = b.messageId < 0 ? Number.MAX_SAFE_INTEGER : b.messageId;
      return ai - bi;
    });

  // Clear the optimistic copy as soon as the server echo lands.
  useEffect(() => {
    if (hasServerEcho) setOptimisticMsg(null);
  }, [hasServerEcho]);

  const isObserved = session?.managed === false;

  // Show a thinking indicator while the agent is working but hasn't streamed
  // any content yet (text/toolcall/reasoning) — cleared once parts arrive.
  const showThinking =
    (agentStatus === "thinking" || sending) && streamParts.length === 0;

  // ── Rotating thinking hints (Claude Code-style) ──────────────────
  const hints = useMemo(
    () => [
      "thinking…",
      "pondering the impossible…",
      "connecting the dots…",
      "channeling the spirits…",
      "consulting the silicon oracle…",
      "bending spacetime…",
      "untangling the thread…",
      "you can queue follow-ups while you wait",
      "tip: Steer on a queued card injects it mid-turn",
      "tip: drag queue cards to reorder",
      "did you know? this server boots in <1s now",
      "pro tip: Shift+Enter for a newline",
    ],
    [],
  );
  const [hintIndex, setHintIndex] = useState(0);
  useEffect(() => {
    if (!showThinking) {
      setHintIndex(0);
      return;
    }
    const timer = setInterval(() => {
      setHintIndex((i) => (i + 1) % hints.length);
    }, 3500);
    return () => clearInterval(timer);
  }, [showThinking, hints.length]);
  const thinkingHint = hints[hintIndex];

  // ── Send (used by composer AND queue auto-drain) ──────────────────
  const doSend = useCallback(
    async (content: string, model?: string, thinking?: string) => {
      setSending(true);
      setAgentStatus("thinking");
      lastSendOpts.current = { model, thinking };

      const now = Math.floor(Date.now() / 1000);
      setOptimisticMsg({
        messageId: -1,
        sessionId,
        role: "user",
        content,
        toolName: null,
        toolCalls: null,
        reasoning: null,
        timestamp: now,
        tokenCount: null,
        finishReason: null,
      });

      try {
        await sendMessage(sessionId, content, model, thinking);
      } catch {
        setSending(false);
        setAgentStatus("idle");
        setOptimisticMsg(null);
        throw new Error("send failed");
      }
    },
    [sessionId],
  );

  // Auto-drain ref so the WS effect always sees the latest queue without
  // re-subscribing on every queue change.
  const queueRef = useRef(queue);
  queueRef.current = queue;
  const doSendRef = useRef(doSend);
  doSendRef.current = doSend;

  // ── WS streaming + status ─────────────────────────────────────────
  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      // Narrow by kind first — not all ServerFrame variants have sessionId.
      if (
        (frame.kind === "message.delta" ||
          frame.kind === "message.done" ||
          frame.kind === "message.toolCall" ||
          frame.kind === "message.reasoning") &&
        frame.sessionId !== sessionId
      ) {
        return;
      }
      if (frame.kind === "message.delta") {
        // Append to the last text part if it's the tail; otherwise push a new one.
        setStreamParts((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.type === "text") {
            return [...prev.slice(0, -1), { type: "text", text: last.text + frame.textDelta }];
          }
          return [...prev, { type: "text", text: frame.textDelta }];
        });
        setAgentStatus("streaming");
      }
      if (frame.kind === "message.toolCall") {
        setStreamParts((prev) => {
          const tc = frame.toolCall;
          // Update in place when we already have this call (matched by id, or
          // by "most recent card without a result" when the id is absent) —
          // this preserves the card's chronological position in the stream.
          const idx =
            tc.id != null
              ? prev.findIndex(
                  (p) => p.type === "toolCall" && p.toolCall.id === tc.id,
                )
              : (() => {
                  for (let i = prev.length - 1; i >= 0; i--) {
                    const p = prev[i];
                    if (p.type === "toolCall" && p.toolCall.result == null) return i;
                  }
                  return -1;
                })();
          if (idx >= 0) {
            const next = [...prev];
            const existing = next[idx] as { type: "toolCall"; toolCall: ToolCall };
            next[idx] = {
              type: "toolCall",
              toolCall: {
                ...existing.toolCall,
                ...tc,
                // Don't let an empty-name update wipe the original name/args.
                name: tc.name || existing.toolCall.name,
                args:
                  tc.args && Object.keys(tc.args as object).length > 0
                    ? tc.args
                    : existing.toolCall.args,
              },
            };
            return next;
          }
          return [...prev, { type: "toolCall", toolCall: tc }];
        });
      }
      if (frame.kind === "message.reasoning") {
        setStreamParts((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.type === "reasoning") {
            return [...prev.slice(0, -1), { type: "reasoning", text: last.text + frame.textDelta }];
          }
          return [...prev, { type: "reasoning", text: frame.textDelta }];
        });
      }
      if (frame.kind === "message.done") {
        setStreamParts([]);
        setSending(false);
        setAgentStatus("done");
        setOptimisticMsg(null);
        setPermission(null);
        // Auto-drain: send the next queued message as a fresh prompt.
        const q = queueRef.current;
        if (q.length > 0) {
          const [head, ...rest] = q;
          setQueue(rest);
          const { model, thinking } = lastSendOpts.current;
          void doSendRef.current(head.text, model, thinking).catch(() => {
            // restore on failure so the message isn't lost
            setQueue((cur) => [head, ...cur]);
          });
        }
      }
      // Liveness pushed from elsewhere (cancel from another tab, server
      // restart marking idle) — keep local state in sync.
      if (frame.kind === "session.updated" && frame.sessionId === sessionId) {
        const lv = (frame.changes as { liveness?: string }).liveness;
        if (lv === "idle") {
          setSending(false);
          setAgentStatus("idle");
          setStreamParts([]);
        }
        if (lv === "running") {
          setSending(true);
          setAgentStatus((s) => (s === "streaming" ? s : "thinking"));
        }
      }
      // Agent is blocked awaiting a permission decision for a gated tool call.
      if (frame.kind === "permission.required" && frame.sessionId === sessionId) {
        setPermission({ toolCall: frame.toolCall, options: frame.options });
      }
      if (frame.kind === "message.appended" && frame.sessionId === sessionId) {
        // Track steer messages as pending until the agent processes them.
        if (frame.message.finishReason === "steer") {
          setPendingSteers((prev) => new Set(prev).add(frame.message.messageId));
        }
      }
      // Steer delivery signal — the agent consumed the steer-ack Done.
      // Clear all pending steers (steers are processed one at a time per turn).
      if (
        frame.kind === "session.log" &&
        frame.sessionId === sessionId &&
        frame.source === "bridge" &&
        frame.message === "steer.delivered"
      ) {
        setPendingSteers(new Set());
      }
      // S8: typing presence from other users in this session.
      if (frame.kind === "user.typing" && frame.sessionId === sessionId) {
        // Don't show our own typing indicator.
        if (frame.who && frame.who !== myName) {
          setTypers((prev) => {
            const next = new Map(prev);
            next.set(frame.who, frame.expiresAt);
            return next;
          });
        }
      }
    });
    return unsub;
  }, [sessionId]);

  const handlePermission = useCallback(
    async (optionId: string | null) => {
      setPermission(null);
      try {
        await respondPermission(sessionId, optionId);
      } catch {
        // If it fails the WS will re-emit or the turn errors; nothing to retry here.
      }
    },
    [sessionId],
  );

  // ── Auto-scroll ──────────────────────────────────────────────────
  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [messages.length, streamParts, agentStatus]);

  // ── Composer send: idle → send now; running → enqueue ─────────────
  const handleSend = useCallback(
    async (model?: string, thinking?: string) => {
      const trimmed = text.trim();
      if (!trimmed) return;
      if (sending) {
        // Turn in flight — queue it (zcode-style follow-up).
        setQueue((cur) => [
          ...cur,
          { id: `q${Date.now()}-${Math.random().toString(36).slice(2, 7)}`, text: trimmed },
        ]);
        setText("");
        return;
      }
      setText("");
      try {
        await doSend(trimmed, model, thinking);
      } catch {
        setText(trimmed); // restore on error
      }
    },
    [text, sending, doSend],
  );

  // ── Stop the running turn (cancel button) ──────────────────────
  const handleStop = useCallback(async () => {
    try {
      await cancelSession(sessionId);
    } catch {
      // ignore — the WS will catch up
    }
    setSending(false);
    setAgentStatus("idle");
    setStreamParts([]);
  }, [sessionId]);

  // ── Queue management ────────────────────────────────────────────
  const handleQueueReorder = useCallback((from: number, to: number) => {
    setQueue((cur) => {
      const next = [...cur];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
  }, []);

  const handleQueueEdit = useCallback((id: string, newText: string) => {
    setQueue((cur) => cur.map((m) => (m.id === id ? { ...m, text: newText } : m)));
  }, []);

  const handleQueueDelete = useCallback((id: string) => {
    setQueue((cur) => cur.filter((m) => m.id !== id));
  }, []);

  /** Steer a QUEUED item into the running turn right now. */
  const handleQueueSteer = useCallback(
    async (id: string) => {
      const item = queueRef.current.find((m) => m.id === id);
      if (!item) return;
      // Optimistically remove; restore on failure.
      setQueue((cur) => cur.filter((m) => m.id !== id));
      try {
        await steerSession(sessionId, item.text);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (msg === "not_running") {
          // No turn in flight — send as a normal prompt instead. The queue
          // auto-drain will handle any remaining items after this one finishes.
          const { model, thinking } = lastSendOpts.current;
          try {
            await doSendRef.current(item.text, model, thinking);
          } catch {
            setQueue((cur) => [item, ...cur]);
          }
        } else {
          // Steer genuinely failed — restore so the user can retry.
          setQueue((cur) => [item, ...cur]);
        }
      }
    },
    [sessionId],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void handleSend();
      }
    },
    [handleSend],
  );

  const handleTextareaInput = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      setText(e.target.value);
      const el = e.target;
      el.style.height = "auto";
      el.style.height = Math.min(el.scrollHeight, 150) + "px";

      // S8: send a typing frame, rate-limited to once per ~3 s so we don't
      // flood the server. The FIRST keystroke sends immediately; subsequent
      // keystrokes within 3 s are suppressed (the server TTL is 5 s so there's
      // no gap). Only sent when there's actual text.
      if (e.target.value.trim()) {
        if (!typingDebounceRef.current) {
          sendFrame({ kind: "typing", sessionId });
          typingDebounceRef.current = setTimeout(() => {
            typingDebounceRef.current = null;
          }, 3000);
        }
      } else {
        // Field cleared — cancel any pending re-enable so the next keystroke
        // sends fresh.
        if (typingDebounceRef.current) {
          clearTimeout(typingDebounceRef.current);
          typingDebounceRef.current = null;
        }
      }
    },
    [sessionId],
  );

  // ── Fork (fixed-position modal) ───────────────────────────────────
  const [forkOpen, setForkOpen] = useState(false);
  const handleForkRequest = useCallback(() => {
    setForkOpen(true);
  }, []);
  const handleForkConfirm = useCallback(async () => {
    setForkOpen(false);
    try {
      const forked = await forkSession(sessionId);
      if (forked?.id) {
        void navigate({
          to: "/sessions/$sessionId",
          params: { sessionId: forked.id },
        });
      }
    } catch {
      // user can retry
    }
    onForkRequested?.(sessionId);
  }, [sessionId, navigate, onForkRequested]);


  return (
    <>
      {/* Transcript — Page content only (no chatcol wrapper; the View provides it) */}
      <div className="transcript" ref={transcriptRef}>
          <div className="tcol">
            {isLoading && (
              <div className="msg-empty">Loading messages…</div>
            )}
            {!isLoading && messages.length === 0 && streamParts.length === 0 && (
              <div className="msg-empty">
                No messages yet. Send a message below.
              </div>
            )}
            {messages.map((m) => (
              <MessageBubble
                key={`${sessionId}-${m.messageId}`}
                msg={m}
                steerPending={pendingSteers.has(m.messageId)}
                onFork={handleForkRequest}
              />
            ))}
            {/* Streaming assistant reply — interleaved text, tool calls, reasoning */}
            {streamParts.length > 0 && (
              <div className="msg-ai">
                {streamParts.map((part, i) => {
                  if (part.type === "toolCall") {
                    return isDiffResult(part.toolCall) ? (
                      <DiffCard key={`tc-${i}`} tc={part.toolCall} />
                    ) : (
                      <ToolCard key={`tc-${i}`} tc={part.toolCall} idx={i} expanded={false} onToggle={() => {}} />
                    );
                  }
                  if (part.type === "reasoning") {
                    return (
                      <div key={`r-${i}`} className="reasoning-block stream-reasoning">
                        <span className="reasoning-toggle gk" style={{ fontSize: 10 }}>
                          thinking
                        </span>
                        <div className="reasoning-body">{part.text}</div>
                      </div>
                    );
                  }
                  return (
                    <ReactMarkdown key={`t-${i}`} remarkPlugins={[remarkGfm]}>
                      {part.text}
                    </ReactMarkdown>
                  );
                })}
              </div>
            )}
            {/* Thinking indicator — agent is working, no text streamed yet.
                Rotating hints/tips in place of a static "thinking…" label. */}
            {showThinking && (
              <div className="msg-ai thinking-row">
                <span className="thinking-dots" aria-label="thinking">
                  <i /><i /><i />
                </span>
                <span className="thinking-label">{thinkingHint}</span>
              </div>
            )}
            {/* S8: user typing indicator — other people typing in this session */}
            {typers.size > 0 && (
              <div className="typing-row">
                <span className="typing-dots" aria-label="typing">
                  <i /><i /><i />
                </span>
                <span className="who-name">
                  {[...typers.keys()].join(", ")} {typers.size === 1 ? "is" : "are"} typing…
                </span>
              </div>
            )}
            {/* Permission prompt — agent blocked on a gated tool call */}
            {permission && (
              <div className="perm-prompt">
                <div className="perm-head">
                  <Icon name="shield" size={13} />
                  <span>
                    Agent wants to run: <strong>{permission.toolCall}</strong>
                  </span>
                </div>
                <div className="perm-opts">
                  {permission.options.map((o) => (
                    <button
                      key={o.optionId}
                      type="button"
                      className={`btn${o.kind.startsWith("allow") ? " pri" : ""}`}
                      onClick={() => void handlePermission(o.optionId)}
                    >
                      {o.name}
                    </button>
                  ))}
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void handlePermission(null)}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      {isObserved ? (
        <div className="composer">
          <div className="obsbanner">
            <Icon name="alert" size={14} />
            <span style={{ flex: 1 }}>
              This is an observed session — read-only. Fork it to continue from Olympus.
            </span>
            <button type="button" className="btn pri" onClick={() => setForkOpen(true)}>
              Fork to continue
            </button>
          </div>
        </div>
      ) : (
        <div className="composer-stack">
          <QueuePanel
            items={queue}
            onReorder={handleQueueReorder}
            onEdit={handleQueueEdit}
            onDelete={handleQueueDelete}
            onSteer={handleQueueSteer}
          />
          <Composer
            text={text}
            onTextChange={handleTextareaInput}
            onKeyDown={handleKeyDown}
            onSend={handleSend}
            onStop={handleStop}
            sending={sending}
            sessionModel={session?.model ?? null}
            sessionAgent={session?.agent ?? null}
            sessionNode={session?.node ?? null}
          />
        </div>
      )}

      {/* Fixed-position fork modal */}
      <ForkModal
        open={forkOpen}
        title="Fork this session?"
        message="A new Olympus-managed session will be created, branching from this point. The original session stays unchanged."
        confirmLabel="Fork to continue"
        onConfirm={handleForkConfirm}
        onCancel={() => setForkOpen(false)}
      />
    </>
  );
}
