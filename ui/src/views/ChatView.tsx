import { useRef, useEffect, useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { useChat } from "../hooks/useChat";
import { useSessions } from "../hooks/useSessions";
import { formatTime, SOURCE_META, formatTokens } from "../lib/format";
import { sendMessage } from "../api";
import type { Message, ToolCall } from "../types";

interface Props {
  sessionId: string;
  onBack: () => void;
}

export default function ChatView({ sessionId, onBack }: Props) {
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
            <span className="chat-managed-badge" title="Olympus-managed — you can steer this session">live</span>
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
      <Composer sessionId={sessionId} managed={managed} sourceLabel={sourceMeta?.label ?? session?.source ?? ""} />
    </div>
  );
}

// ── Composer ───────────────────────────────────────────
function Composer({ sessionId, managed, sourceLabel }: { sessionId: string; managed: boolean; sourceLabel: string }) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

  const autosize = useCallback(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 160) + "px";
  }, []);

  const submit = useCallback(async () => {
    const body = text.trim();
    if (!body || sending) return;
    setSending(true);
    setErr(null);
    try {
      await sendMessage(sessionId, body);
      setText("");
      if (taRef.current) taRef.current.style.height = "auto";
    } catch (e) {
      setErr(String(e));
    } finally {
      setSending(false);
    }
  }, [text, sending, sessionId]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  if (!managed) {
    return (
      <div className="composer composer-locked">
        <div className="composer-locked-text">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />
          </svg>
          This is an observed <strong>{sourceLabel}</strong> session — read-only. Fork it to continue from Olympus.
        </div>
        <button className="composer-fork-btn" disabled title="Fork — coming with the ACP bridge">
          Fork to continue
        </button>
      </div>
    );
  }

  return (
    <div className="composer">
      {err && <div className="composer-error">{err}</div>}
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
