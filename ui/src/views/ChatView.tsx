import { useRef, useEffect, useState, useCallback, memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { useChat } from "../hooks/useChat";
import { useSessions } from "../hooks/useSessions";
import { formatTime, SOURCE_META, formatTokens } from "../lib/format";
import { forkSession, sendMessage, updateSession, fetchAgents, cancelSession } from "../api";
import type { Message, Session, ToolCall, AgentInfo } from "../types";

interface Props {
  sessionId: string;
  onBack: () => void;
  onOpenSession: (id: string) => void;
}

// Stable reference (never re-created) so useSessions' effect doesn't refire.
const SESSION_META_PARAMS = {};

export default function ChatView({ sessionId, onBack, onOpenSession }: Props) {
  const { messages, loading, error, streamingText, streamingIds, loadOlder, hasOlder } = useChat(sessionId);
  // Stable empty-params object so useSessions doesn't re-subscribe every render
  // (an inline {} is a new reference each render → effect re-fires → lag).
  const sessionMeta = useSessions(SESSION_META_PARAMS);
  const session = sessionMeta.sessions.find((s) => s.id === sessionId);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoFollow, setAutoFollow] = useState(true);

  // Show the "thinking…" indicator when a turn is in-flight but no assistant
  // text is streaming yet: the session is active (liveness), nothing is
  // currently streaming, and the most recent message is the user's. This bridges
  // the silent gap between pressing send and the first token (can be 10-120s on
  // a cold spawn or a rate-limited provider).
  const lastMsg = messages[messages.length - 1];
  const showThinking =
    session?.liveness === "active" &&
    streamingIds.size === 0 &&
    !!lastMsg &&
    lastMsg.role === "user";

  const scrollToBottom = useCallback(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, []);

  useEffect(() => {
    if (autoFollow) scrollToBottom();
  }, [messages, streamingText, autoFollow, scrollToBottom]);

  useEffect(() => {
    setAutoFollow(true);
    scrollToBottom();
  }, [sessionId, scrollToBottom]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    setAutoFollow(atBottom);
    if (el.scrollTop < 50 && hasOlder) {
      loadOlder();
    }
  };

  const sourceMeta = session ? SOURCE_META[session.source] : null;
  const managed = session?.managed ?? false;

  return (
    <div className="chat-view" data-session-id={sessionId}>
      {/* Header */}
      <div className="chat-header">
        <button className="back-btn" onClick={onBack}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="m15 18-6-6 6-6" />
          </svg>
          Sessions
        </button>
        <div className="chat-title">
          <span>{session?.title ?? "(no title)"}</span>
          {sourceMeta && (
            <span className="chat-source-badge" style={{ color: sourceMeta.color, borderColor: sourceMeta.color }}>
              {sourceMeta.label}
            </span>
          )}
          {session?.model && <span className="chat-model-pill">{session.model}</span>}
          {managed ? (
            session?.liveness === "active" ? (
              <span className="chat-live-badge" title="Active — a turn is in-flight or recent activity">
                <span className="chat-live-dot" />running
              </span>
            ) : (
              <span className="chat-managed-badge" title="Olympus-managed — idle, ready for your next message">idle</span>
            )
          ) : (
            <span className="chat-observed-badge" title="Observed session — read-only. Fork to continue it from Olympus.">observed</span>
          )}
        </div>
        <div className="chat-stats">
          {session && <span>{session.messageCount} msgs · {formatTokens(session.inputTokens + session.outputTokens)} tok</span>}
        </div>
      </div>

      {/* Messages */}
      <div className="chat-scroll" ref={scrollRef} onScroll={handleScroll}>
        {loading && <TranscriptSkeleton />}
        {error && <div className="chat-error">{error}</div>}
        {!loading && messages.length === 0 && <div className="chat-empty">No messages in this session.</div>}
        {messages.map((msg) => (
          <MessageBubble key={msg.messageId} message={msg} streamingText={streamingText[msg.messageId]} />
        ))}
        {/* Waiting indicator: the agent is working but hasn't started streaming
            text yet. Shows after a send (session active) when the last message
            is the user's and nothing is currently streaming. This is the
            difference between "feels frozen" and "I can see it's thinking". */}
        {showThinking && (
          <div className="msg msg-assistant">
            <div className="msg-gutter"><span className="role-badge role-ai">AI</span></div>
            <div className="msg-body">
              <div className="thinking-indicator" aria-label="Agent is working">
                <span className="thinking-dot" />
                <span className="thinking-dot" />
                <span className="thinking-dot" />
                <span className="thinking-label">thinking…</span>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Jump to latest */}
      {!autoFollow && (
        <button className="jump-latest" onClick={scrollToBottom}>
          Jump to latest ↓
        </button>
      )}

      {/* Composer */}
      <Composer
        sessionId={sessionId}
        managed={managed}
        agent={session?.agent ?? null}
        model={session?.model ?? null}
        hermesId={session?.hermesId ?? ""}
        sourceLabel={sourceMeta?.label ?? session?.source ?? ""}
        inFlight={session?.liveness === "active"}
        onAssigned={() => sessionMeta.refetch()}
        onForked={(forked) => {
          sessionMeta.refetch();
          onOpenSession(forked.id);
        }}
      />
    </div>
  );
}

// Live agent list from the backend (/api/agents → real Hermes profiles with
// their configured provider + model). Falls back to a minimal default-only list
// if the fetch fails so the picker never renders empty.
function useAgents(): AgentInfo[] {
  const [agents, setAgents] = useState<AgentInfo[]>([
    { id: "", provider: null, model: null, isDefault: true },
  ]);
  useEffect(() => {
    let alive = true;
    fetchAgents()
      .then((r) => {
        if (!alive) return;
        // Map the root "default" agent to value "" (the no-agent draft default).
        const list = r.agents.map((a) =>
          a.isDefault ? { ...a, id: "" } : a
        );
        setAgents(list);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  return agents;
}

// ── Composer ───────────────────────────────────────────
function Composer({
  sessionId,
  managed,
  agent,
  model,
  hermesId,
  sourceLabel,
  inFlight,
  onAssigned,
  onForked,
}: {
  sessionId: string;
  managed: boolean;
  agent: string | null;
  model: string | null;
  hermesId: string;
  sourceLabel: string;
  inFlight: boolean;
  onAssigned: () => void;
  onForked: (session: Session) => void;
}) {
  const [text, setText] = useState("");
  const [draftModel, setDraftModel] = useState<string | null>(model);
  const [sending, setSending] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [forking, setForking] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const agents = useAgents();

  // Keep the draft synced when the prop changes (e.g. agent pick auto-fills it).
  useEffect(() => { setDraftModel(model); }, [model]);
  const onModelChange = useCallback((v: string) => setDraftModel(v), []);

  // The runtime is live once a Hermes id has been captured (after first send).
  // Before that the agent/model are still re-bindable.
  const bound = hermesId !== "";

  const autosize = useCallback(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 160) + "px";
  }, []);

  const assign = useCallback(
    async (patch: { agent?: string; model?: string }) => {
      setErr(null);
      try {
        await updateSession(sessionId, patch);
        onAssigned();
      } catch (e) {
        setErr(String(e));
      }
    },
    [sessionId, onAssigned]
  );

  const submit = useCallback(async () => {
    const body = text.trim();
    if (!body || sending) return;
    setSending(true);
    setErr(null);
    try {
      await sendMessage(sessionId, body);
      setText("");
      if (taRef.current) taRef.current.style.height = "auto";
      onAssigned(); // refresh so the captured hermesId / "live" badge shows
    } catch (e) {
      setErr(String(e));
    } finally {
      setSending(false);
    }
  }, [text, sending, sessionId, onAssigned]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  const stop = useCallback(async () => {
    if (stopping) return;
    setStopping(true);
    setErr(null);
    try {
      await cancelSession(sessionId);
      onAssigned(); // refresh liveness so the indicator clears
    } catch (e) {
      setErr(String(e));
    } finally {
      setStopping(false);
    }
  }, [stopping, sessionId, onAssigned]);

  const fork = useCallback(async () => {
    if (forking) return;
    setForking(true);
    setErr(null);
    try {
      onForked(await forkSession(sessionId));
    } catch (e) {
      setErr(String(e));
    } finally {
      setForking(false);
    }
  }, [forking, onForked, sessionId]);

  if (!managed) {
    return (
      <div className="composer composer-locked">
        {err && <div className="composer-error">{err}</div>}
        <div className="composer-locked-text">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />
          </svg>
          This is an observed <strong>{sourceLabel}</strong> session — read-only. Fork it to continue from Olympus.
        </div>
        <button className="composer-fork-btn" disabled={forking} onClick={fork}>
          {forking ? "Forking…" : "Fork to continue"}
        </button>
      </div>
    );
  }

  return (
    <div className="composer">
      {err && <div className="composer-error">{err}</div>}
      <div className="composer-assign-row">
        <label className="composer-assign">
          <span className="composer-assign-label">Agent</span>
          <select
            className="composer-assign-select"
            value={agent ?? ""}
            disabled={bound}
            title={bound ? "Agent is locked once the session is live" : "Pick the agent that drives this session"}
            onChange={(e) => {
              // When picking an agent, also adopt its default model so the
              // Model field reflects what will actually run (operator can override).
              const picked = agents.find((a) => a.id === e.target.value);
              assign({ agent: e.target.value, model: picked?.model ?? undefined });
            }}
          >
            {agents.map((a) => {
              const label = a.id === "" ? "Default agent" : a.id;
              const suffix =
                a.provider || a.model
                  ? ` — ${[a.provider, a.model].filter(Boolean).join(" / ")}`
                  : "";
              return (
                <option key={a.id || "__default"} value={a.id}>
                  {label}
                  {suffix}
                </option>
              );
            })}
          </select>
        </label>
        <label className="composer-assign">
          <span className="composer-assign-label">Model</span>
          <input
            className="composer-assign-input"
            type="text"
            placeholder="default"
            value={draftModel ?? ""}
            disabled={bound}
            title={bound ? "Model is locked once the session is live" : "Optional model override (e.g. glm-5.2)"}
            // PATCH on blur, not every keystroke — onChange-per-key fires a
            // server round-trip on each char and was a typing-lag contributor.
            onChange={(e) => onModelChange(e.target.value)}
            onBlur={() => { if (draftModel !== (model ?? "")) assign({ model: draftModel ?? undefined }); }}

          />
        </label>
        {bound && <span className="composer-assign-locked">runtime live · binding locked</span>}
      </div>
      <div className="composer-input-row">
        <textarea
          ref={taRef}
          className="composer-input"
          placeholder="Message this session…  (Enter to send, Shift+Enter for newline)"
          value={text}
          rows={1}
          disabled={sending}
          onChange={(e) => { setText(e.target.value); autosize(); }}
          onKeyDown={onKeyDown}
        />
        {inFlight ? (
          <button
            className="composer-stop"
            onClick={stop}
            disabled={stopping}
            title="Stop the agent's current turn"
          >
            {stopping ? (
              <span className="composer-spinner" />
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                <rect x="6" y="6" width="12" height="12" rx="2" />
              </svg>
            )}
          </button>
        ) : (
          <button className="composer-send" onClick={submit} disabled={!text.trim() || sending}>
            {sending ? (
              <span className="composer-spinner" />
            ) : (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="m22 2-7 20-4-9-9-4Z" /><path d="M22 2 11 13" />
              </svg>
            )}
          </button>
        )}
      </div>
    </div>
  );
}

// ── Loading skeleton ───────────────────────────────────
function TranscriptSkeleton() {
  return (
    <div className="transcript-skeleton" aria-label="Loading conversation">
      {[0, 1, 2].map((i) => (
        <div key={i} className={`skel-msg ${i % 2 === 0 ? "skel-user" : "skel-ai"}`}>
          <div className="skel-gutter" />
          <div className="skel-lines">
            <div className="skel-line" style={{ width: "40%" }} />
            <div className="skel-line" style={{ width: i % 2 === 0 ? "65%" : "92%" }} />
            <div className="skel-line" style={{ width: i % 2 === 0 ? "0" : "78%" }} />
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Message rendering ──────────────────────────────────
const ROLE_META: Record<string, { label: string; cls: string }> = {
  user: { label: "You", cls: "role-user" },
  assistant: { label: "AI", cls: "role-ai" },
  tool: { label: "Tool", cls: "role-tool" },
  system: { label: "System", cls: "role-system" },
  session_meta: { label: "Event", cls: "role-meta" },
};

// Memoized: the bubble runs ReactMarkdown + SyntaxHighlighter on every render,
// which is expensive. Without memo, typing in the Composer re-renders ChatView
// and re-parses markdown for EVERY message on each keystroke → typing lag.
// memo() short-circuits when props are unchanged (a non-streaming message's
// props are stable, so it only re-renders when its own content changes).
const MessageBubble = memo(function MessageBubble({ message, streamingText }: { message: Message; streamingText?: string }) {
  const [showReasoning, setShowReasoning] = useState(false);
  const content = streamingText ?? message.content;
  const role = ROLE_META[message.role] ?? { label: message.role, cls: "role-unknown" };

  // session_meta + system render as a centered divider, not a chat bubble —
  // they're lifecycle markers (compaction, model switch), not conversation.
  if (message.role === "system" || message.role === "session_meta") {
    return (
      <div className="msg-divider">
        <span className="msg-divider-line" />
        <span className="msg-divider-label">
          {message.role === "session_meta" ? "session event" : "system"}
          {content ? `: ${truncate(content, 80)}` : ""}
          <span className="msg-divider-time"> · {formatTime(message.timestamp)}</span>
        </span>
        <span className="msg-divider-line" />
      </div>
    );
  }

  // A pure tool-result message (role=tool, no toolCalls of its own).
  if (message.role === "tool") {
    return (
      <div className={`msg msg-tool`}>
        <div className="msg-gutter">
          <RoleBadge role={role} />
        </div>
        <div className="msg-body">
          {message.toolName && <div className="tool-result-name">{message.toolName}</div>}
          {content && <pre className="tool-result-output">{truncate(content, 4000)}</pre>}
          <div className="msg-meta">{formatTime(message.timestamp)}</div>
        </div>
      </div>
    );
  }

  return (
    <div className={`msg msg-${message.role}`}>
      <div className="msg-gutter">
        <RoleBadge role={role} />
      </div>
      <div className="msg-body">
        {message.reasoning && (
          <div className="reasoning-block">
            <button className="reasoning-toggle" onClick={() => setShowReasoning(!showReasoning)}>
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
                style={{ transform: showReasoning ? "rotate(90deg)" : "none", transition: "transform .15s" }}>
                <path d="m9 18 6-6-6-6" />
              </svg>
              reasoning
            </button>
            {showReasoning && <div className="reasoning-content">{message.reasoning}</div>}
          </div>
        )}
        {message.toolCalls && message.toolCalls.length > 0 && (
          <div className="tool-calls">
            {message.toolCalls.map((tc, i) => (
              <ToolCallCard key={i} tc={tc} />
            ))}
          </div>
        )}
        {content && (
          <div className="msg-content">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || "");
                  const code = String(children).replace(/\n$/, "");
                  if (match) {
                    return (
                      <SyntaxHighlighter
                        language={match[1]}
                        style={vscDarkPlus}
                        customStyle={{ margin: 0, borderRadius: "6px", fontSize: "13px" }}
                      >
                        {code}
                      </SyntaxHighlighter>
                    );
                  }
                  return <code className="inline-code" {...props}>{children}</code>;
                },
              }}
            >
              {content}
            </ReactMarkdown>
          </div>
        )}
        <div className="msg-meta">
          {message.tokenCount ? `${message.tokenCount} tokens · ` : ""}{formatTime(message.timestamp)}
          {streamingText !== undefined && <span className="streaming-tag"> · streaming…</span>}
        </div>
      </div>
    </div>
  );
});

function RoleBadge({ role }: { role: { label: string; cls: string } }) {
  return <span className={`role-badge ${role.cls}`}>{role.label}</span>;
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + "…" : s;
}

function ToolCallCard({ tc }: { tc: ToolCall }) {
  const [expanded, setExpanded] = useState(false);
  // result === null while in-flight; undefined means we just don't have one.
  const isRunning = tc.result === null || tc.result === undefined;

  // Args arrive already-parsed (object) from the backend; stringify for display.
  const argsStr = formatValue(tc.args);
  // Detect a patch/edit/diff-shaped result so we render a proper diff view
  // instead of a raw wall of text. A result is "diff-shaped" if it contains
  // unified-diff hunks (lines starting with +/-/@@) or looks like a patch.
  const result = tc.result ?? null;
  const isDiff = result != null && looksLikeDiff(result);

  return (
    <div className="tool-call-card">
      <div className="tool-call-header" onClick={() => setExpanded(!expanded)}>
        <span className={`tool-status ${isRunning ? "running" : "done"}`}>
          {isRunning ? "◌" : "✓"}
        </span>
        <span className="tool-name">{tc.label || tc.name}</span>
        {isRunning && <span className="tool-running-label">running</span>}
        <svg className="tool-chevron" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
          style={{ transform: expanded ? "rotate(90deg)" : "none", transition: "transform .15s" }}>
          <path d="m9 18 6-6-6-6" />
        </svg>
      </div>
      {expanded && (
        <div className="tool-call-detail">
          <div className="tool-section">
            <div className="tool-section-label">args</div>
            <pre className="tool-json">{argsStr}</pre>
          </div>
          {result != null && (
            <div className="tool-section">
              <div className="tool-section-label">{isDiff ? "diff" : "result"}</div>
              {isDiff ? <DiffView diff={result} /> : <pre className="tool-result">{result}</pre>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Format an already-parsed value (object/string/etc.) for display. */
function formatValue(v: unknown): string {
  if (v == null) return "";
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

/** Heuristic: does this tool result look like a unified diff / patch? */
function looksLikeDiff(s: string): boolean {
  // Needs diff/change markers on a meaningful fraction of lines.
  const lines = s.split("\n");
  if (lines.length < 3) return false;
  let markers = 0;
  for (const ln of lines) {
    if (ln.startsWith("+") || ln.startsWith("-") || ln.startsWith("@@") || ln.startsWith("diff ")) {
      markers++;
    }
  }
  return markers >= 2;
}

/** Render a unified-diff string as red/green line-by-line. */
function DiffView({ diff }: { diff: string }) {
  const lines = diff.split("\n");
  return (
    <div className="tool-diff">
      {lines.map((ln, i) => {
        let cls = "diff-line ctx";
        if (ln.startsWith("@@")) cls = "diff-line hunk";
        else if (ln.startsWith("+++") || ln.startsWith("---")) cls = "diff-line meta";
        else if (ln.startsWith("+")) cls = "diff-line add";
        else if (ln.startsWith("-")) cls = "diff-line del";
        return (
          <div key={i} className={cls}>
            <span className="diff-gutter">{ln ? ln[0] : " "}</span>
            <span className="diff-text">{ln.replace(/^[+\- ]/, "")}</span>
          </div>
        );
      })}
    </div>
  );
}
