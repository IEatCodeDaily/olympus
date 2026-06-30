import { useRef, useEffect, useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { useChat } from "../hooks/useChat";
import { useSessions } from "../hooks/useSessions";
import { formatTime, SOURCE_META, formatTokens } from "../lib/format";
import { forkSession, sendMessage, updateSession } from "../api";
import type { Message, Session, ToolCall } from "../types";

interface Props {
  sessionId: string;
  onBack: () => void;
  onOpenSession: (id: string) => void;
}

export default function ChatView({ sessionId, onBack, onOpenSession }: Props) {
  const { messages, loading, error, streamingText, loadOlder, hasOlder } = useChat(sessionId);
  const sessionMeta = useSessions({});
  const session = sessionMeta.sessions.find((s) => s.id === sessionId);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoFollow, setAutoFollow] = useState(true);

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
        onAssigned={() => sessionMeta.refetch()}
        onForked={(forked) => {
          sessionMeta.refetch();
          onOpenSession(forked.id);
        }}
      />
    </div>
  );
}

// Known agent profiles operators can assign (Hermes profiles). Kept small and
// explicit for the MVP; a future /api/agents endpoint can populate this live.
const AGENT_OPTIONS = [
  { value: "", label: "Default agent" },
  { value: "coding-agent", label: "coding-agent" },
  { value: "glm52", label: "glm52" },
  { value: "gpt55", label: "gpt55" },
  { value: "tester", label: "tester" },
  { value: "design-lead", label: "design-lead" },
];

// ── Composer ───────────────────────────────────────────
function Composer({
  sessionId,
  managed,
  agent,
  model,
  hermesId,
  sourceLabel,
  onAssigned,
  onForked,
}: {
  sessionId: string;
  managed: boolean;
  agent: string | null;
  model: string | null;
  hermesId: string;
  sourceLabel: string;
  onAssigned: () => void;
  onForked: (session: Session) => void;
}) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [forking, setForking] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

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
            onChange={(e) => assign({ agent: e.target.value })}
          >
            {AGENT_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </select>
        </label>
        <label className="composer-assign">
          <span className="composer-assign-label">Model</span>
          <input
            className="composer-assign-input"
            type="text"
            placeholder="default"
            defaultValue={model ?? ""}
            disabled={bound}
            title={bound ? "Model is locked once the session is live" : "Optional model override (e.g. glm-5.2)"}
            onBlur={(e) => {
              const v = e.target.value.trim();
              if (v && v !== (model ?? "")) assign({ model: v });
            }}
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
        <button className="composer-send" onClick={submit} disabled={!text.trim() || sending}>
          {sending ? (
            <span className="composer-spinner" />
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="m22 2-7 20-4-9-9-4Z" /><path d="M22 2 11 13" />
            </svg>
          )}
        </button>
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

function MessageBubble({ message, streamingText }: { message: Message; streamingText?: string }) {
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
}

function RoleBadge({ role }: { role: { label: string; cls: string } }) {
  return <span className={`role-badge ${role.cls}`}>{role.label}</span>;
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + "…" : s;
}

function ToolCallCard({ tc }: { tc: ToolCall }) {
  const [expanded, setExpanded] = useState(false);
  const isRunning = tc.result === null;

  let parsedArgs: unknown = tc.args;
  try {
    parsedArgs = JSON.parse(tc.args);
  } catch {
    // keep raw
  }

  return (
    <div className="tool-call-card">
      <div className="tool-call-header" onClick={() => setExpanded(!expanded)}>
        <span className={`tool-status ${isRunning ? "running" : "done"}`}>
          {isRunning ? "◌" : "✓"}
        </span>
        <span className="tool-name">{tc.name}</span>
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
            <pre className="tool-json">{JSON.stringify(parsedArgs, null, 2)}</pre>
          </div>
          {tc.result && (
            <div className="tool-section">
              <div className="tool-section-label">result</div>
              <pre className="tool-result">{tc.result}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
