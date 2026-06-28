import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import type { Message, ToolCall, Session, ServerFrame } from "../types";
import { fetchMessages, fetchSession } from "../api";
import { connectWs, onFrame, sendFrame } from "../api";
import { sourceColor, sourceLabel, relativeTime, formatTokens, ModelPill, Chevron, Spinner } from "../components";

interface Props {
  sessionId: string;
}

export function ChatView({ sessionId }: Props) {
  const [session, setSession] = useState<Session | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [autoFollow, setAutoFollow] = useState(true);
  const [showJumpButton, setShowJumpButton] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  const messagesRef = useRef<Message[]>([]);
  messagesRef.current = messages;

  // Load session + messages
  const loadSession = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [sess, msgsResp] = await Promise.all([
        fetchSession(sessionId),
        fetchMessages(sessionId),
      ]);
      setSession(sess);
      setMessages(msgsResp.messages);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load session");
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    loadSession();
  }, [loadSession]);

  // WebSocket: subscribe to this session's message stream
  useEffect(() => {
    connectWs();
    sendFrame({ kind: "subscribe", sessionId });

    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "message.appended" && frame.sessionId === sessionId) {
        setMessages((prev) => [...prev, frame.message]);
      } else if (frame.kind === "message.delta" && frame.sessionId === sessionId) {
        setMessages((prev) =>
          prev.map((m) =>
            m.messageId === frame.messageId && m.content !== null
              ? { ...m, content: m.content + frame.textDelta }
              : m
          )
        );
      } else if (frame.kind === "message.done" && frame.sessionId === sessionId) {
        setMessages((prev) =>
          prev.map((m) =>
            m.messageId === frame.messageId
              ? { ...m, finishReason: frame.finishReason }
              : m
          )
        );
      } else if (frame.kind === "session.updated" && frame.sessionId === sessionId) {
        setSession((prev) => prev ? { ...prev, ...frame.changes } : prev);
      }
    });

    return () => {
      sendFrame({ kind: "unsubscribe", sessionId });
      unsub();
    };
  }, [sessionId]);

  // Auto-follow: scroll to bottom when new messages arrive
  useEffect(() => {
    if (autoFollow && scrollRef.current) {
      requestAnimationFrame(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      });
    }
  }, [messages, autoFollow]);

  // Detect scroll position
  const handleScroll = useCallback(() => {
    if (!scrollRef.current) return;
    const el = scrollRef.current;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    setAutoFollow(atBottom);
    setShowJumpButton(!atBottom && messages.length > 0);

    // Load older messages on scroll to top
    if (el.scrollTop < 100 && !loadingMore && messages.length > 0) {
      loadOlderMessages();
    }
  }, [messages, loadingMore]);

  const loadOlderMessages = useCallback(async () => {
    if (!session) return;
    setLoadingMore(true);
    try {
      // Simulate pagination — in production this would use a cursor
      const resp = await fetchMessages(sessionId, { limit: 50 });
      if (resp.messages.length > messagesRef.current.length) {
        // Prepend older messages
        const newMsgs = resp.messages.filter(
          (m) => !messagesRef.current.some((existing) => existing.messageId === m.messageId)
        );
        if (newMsgs.length > 0) {
          const prevHeight = scrollRef.current?.scrollHeight ?? 0;
          setMessages((prev) => [...newMsgs, ...prev]);
          // Maintain scroll position
          requestAnimationFrame(() => {
            if (scrollRef.current) {
              const newHeight = scrollRef.current.scrollHeight;
              scrollRef.current.scrollTop = newHeight - prevHeight;
            }
          });
        }
      }
    } catch {
      // Silent — pagination is best-effort
    } finally {
      setLoadingMore(false);
    }
  }, [sessionId, session]);

  const jumpToLatest = () => {
    setAutoFollow(true);
    if (scrollRef.current) {
      scrollRef.current.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
    }
  };

  if (loading) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%" }}>
        <Spinner size={18} />
      </div>
    );
  }

  if (error || !session) {
    return (
      <div style={{ padding: "40px", textAlign: "center", color: "var(--error)", fontSize: "13px" }}>
        {error ?? "Session not found"}
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* ── Header ── */}
      <ChatHeader session={session} />

      {/* ── Messages ── */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        style={{
          flex: 1,
          overflow: "auto",
          position: "relative",
        }}
      >
        {loadingMore && (
          <div style={{ textAlign: "center", padding: "12px", fontSize: "11px", color: "var(--text-faint)" }}>
            <Spinner size={12} /> Loading older…
          </div>
        )}

        <div style={{ maxWidth: "760px", margin: "0 auto", padding: "24px 32px 120px" }}>
          {messages.length === 0 ? (
            <div style={{
              textAlign: "center",
              padding: "60px 0",
              color: "var(--text-faint)",
              fontSize: "13px",
            }}>
              No messages in this session.
            </div>
          ) : (
            messages.map((msg, i) => (
              <MessageBubble
                key={`${msg.messageId}-${i}`}
                message={msg}
                isLast={i === messages.length - 1}
              />
            ))
          )}
        </div>

        {/* Jump to latest button */}
        {showJumpButton && (
          <button
            onClick={jumpToLatest}
            style={{
              position: "absolute",
              bottom: "20px",
              left: "50%",
              transform: "translateX(-50%)",
              display: "flex",
              alignItems: "center",
              gap: "5px",
              padding: "5px 12px",
              fontSize: "11px",
              fontFamily: "var(--font-mono)",
              color: "var(--text-secondary)",
              background: "var(--bg-glass)",
              backdropFilter: "blur(8px)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-lg)",
              cursor: "pointer",
              animation: "fadeInUp var(--dur-fast) var(--ease-spring)",
            }}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path d="M5 8V2M5 2L2 5M5 2L8 5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            </svg>
            Latest
          </button>
        )}
      </div>
    </div>
  );
}

/* ── Chat header bar ── */
function ChatHeader({ session }: { session: Session }) {
  const title = session.title ?? "Untitled";
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      gap: "10px",
      height: "var(--header-h)",
      padding: "0 20px",
      borderBottom: "1px solid var(--border-faint)",
      background: "var(--bg-1)",
      flexShrink: 0,
    }}>
      <span
        style={{
          width: "7px",
          height: "7px",
          borderRadius: "50%",
          background: sourceColor(session.source),
          flexShrink: 0,
        }}
      />
      <span style={{
        fontSize: "13px",
        fontWeight: 500,
        color: "var(--text-primary)",
        flex: 1,
      }} className="truncate">
        {title}
      </span>
      {session.model && <ModelPill model={session.model} />}
      <span className="mono" style={{ fontSize: "10px", color: "var(--text-faint)" }}>
        {sourceLabel(session.source)}
      </span>
      <span className="mono" style={{ fontSize: "10px", color: "var(--text-faint)" }}>
        · {relativeTime(session.lastActivity)}
      </span>
      {session.managed && (
        <span className="mono" style={{
          fontSize: "9px",
          fontWeight: 500,
          color: "var(--accent)",
          textTransform: "uppercase",
          letterSpacing: "0.05em",
        }}>
          Managed
        </span>
      )}
    </div>
  );
}

/* ── Message bubble ── */
function MessageBubble({ message, isLast }: { message: Message; isLast: boolean }) {
  const isUser = message.role === "user";
  const isAssistant = message.role === "assistant";
  const isTool = message.role === "tool";
  const isSystem = message.role === "system";

  // Skip rendering empty tool/system messages
  if (isTool && !message.content) return null;

  const label = isUser ? "You" : isAssistant ? "Assistant" : isTool ? `Tool: ${message.toolName}` : "System";
  const labelColor = isUser ? "var(--text-secondary)" : isAssistant ? "var(--accent)" : isTool ? "var(--warning)" : "var(--text-faint)";

  return (
    <div
      className="animate-in"
      style={{
        marginBottom: "24px",
        animationDelay: isLast ? "0ms" : "0ms",
      }}
    >
      {/* Role label */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: "8px",
        marginBottom: "6px",
      }}>
        <span className="mono" style={{
          fontSize: "10.5px",
          fontWeight: 500,
          color: labelColor,
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}>
          {label}
        </span>
        <span className="mono" style={{
          fontSize: "10px",
          color: "var(--text-faint)",
        }}>
          {relativeTime(message.timestamp)}
        </span>
        {message.tokenCount && (
          <span className="mono" style={{
            fontSize: "9.5px",
            color: "var(--text-faint)",
            opacity: 0.7,
          }}>
            · {formatTokens(message.tokenCount)} tok
          </span>
        )}
      </div>

      {/* Reasoning block (collapsible) */}
      {message.reasoning && (
        <CollapsibleBlock
          label="Reasoning"
          color="var(--text-tertiary)"
          icon="◇"
          defaultOpen={false}
        >
          <div style={{
            padding: "10px 14px",
            fontSize: "12px",
            lineHeight: "1.6",
            color: "var(--text-tertiary)",
            fontFamily: "var(--font-mono)",
            whiteSpace: "pre-wrap",
          }}>
            {message.reasoning}
          </div>
        </CollapsibleBlock>
      )}

      {/* Tool calls (collapsible cards) */}
      {message.toolCalls && message.toolCalls.length > 0 && (
        <div style={{ marginBottom: "8px" }}>
          {message.toolCalls.map((tc, i) => (
            <ToolCallCard key={i} call={tc} />
          ))}
        </div>
      )}

      {/* Content */}
      {message.content !== null && (
        isUser ? (
          <div style={{
            padding: "12px 16px",
            background: "var(--bg-2)",
            borderRadius: "var(--radius-lg)",
            border: "1px solid var(--border-faint)",
            fontSize: "13.5px",
            lineHeight: "1.65",
            color: "var(--text-primary)",
          }}>
            {message.content}
          </div>
        ) : isTool ? (
          <div style={{
            padding: "10px 14px",
            background: "var(--warning-dim)",
            borderRadius: "var(--radius-md)",
            border: "1px solid rgba(251, 191, 36, 0.15)",
            fontSize: "11.5px",
            lineHeight: "1.5",
            color: "var(--text-secondary)",
            fontFamily: "var(--font-mono)",
            whiteSpace: "pre-wrap",
            overflowX: "auto",
          }}>
            {message.content}
          </div>
        ) : (
          <div className="markdown-body" style={{
            fontSize: "13.5px",
            lineHeight: "1.7",
            color: "var(--text-primary)",
          }}>
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ node, className, children, ...props }: any) {
                  const match = /language-(\w+)/.exec(className || "");
                  const isInline = !match && !String(children).includes("\n");
                  return isInline ? (
                    <code
                      {...props}
                      style={{
                        fontFamily: "var(--font-mono)",
                        fontSize: "12px",
                        padding: "1px 4px",
                        borderRadius: "3px",
                        background: "var(--bg-3)",
                        color: "var(--accent-hover)",
                      }}
                    >
                      {children}
                    </code>
                  ) : (
                    <SyntaxHighlighter
                      {...props}
                      style={vscDarkPlus as any}
                      language={match?.[1] ?? "text"}
                      PreTag="div"
                      customStyle={{
                        margin: "0 0 12px 0",
                        borderRadius: "var(--radius-md)",
                        border: "1px solid var(--border-faint)",
                        background: "var(--bg-0)",
                        fontSize: "12px",
                      }}
                    >
                      {String(children).replace(/\n$/, "")}
                    </SyntaxHighlighter>
                  );
                },
              }}
            >
              {message.content}
            </ReactMarkdown>
            {/* Streaming cursor */}
            {isAssistant && message.finishReason === null && (
              <span style={{
                display: "inline-block",
                width: "7px",
                height: "15px",
                background: "var(--accent)",
                marginLeft: "2px",
                animation: "blink 1s step-end infinite",
                verticalAlign: "text-bottom",
              }} />
            )}
          </div>
        )
      )}
    </div>
  );
}

/* ── Collapsible block (reasoning, tool details) ── */
function CollapsibleBlock({
  label,
  color,
  icon,
  defaultOpen,
  children,
}: {
  label: string;
  color: string;
  icon: string;
  defaultOpen: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div style={{
      marginBottom: "8px",
      borderRadius: "var(--radius-md)",
      border: "1px solid var(--border-faint)",
      background: "var(--bg-1)",
      overflow: "hidden",
    }}>
      <button
        onClick={() => setOpen(!open)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: "6px",
          width: "100%",
          padding: "6px 10px",
          background: "transparent",
          border: "none",
          cursor: "pointer",
          color,
          fontSize: "10.5px",
          fontFamily: "var(--font-mono)",
          fontWeight: 500,
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        <Chevron open={open} />
        <span style={{ opacity: 0.7 }}>{icon}</span>
        {label}
      </button>
      {open && <div>{children}</div>}
    </div>
  );
}

/* ── Tool call card ── */
function ToolCallCard({ call }: { call: ToolCall }) {
  const [open, setOpen] = useState(false);
  const isRunning = call.result === null;

  let prettyArgs = call.args;
  try {
    prettyArgs = JSON.stringify(JSON.parse(call.args), null, 2);
  } catch { /* keep raw */ }

  return (
    <div style={{
      borderRadius: "var(--radius-md)",
      border: "1px solid var(--border-faint)",
      background: "var(--bg-1)",
      overflow: "hidden",
      marginBottom: "6px",
    }}>
      <button
        onClick={() => setOpen(!open)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          width: "100%",
          padding: "7px 10px",
          background: "transparent",
          border: "none",
          cursor: "pointer",
          textAlign: "left",
        }}
      >
        <Chevron open={open} />
        <span className="mono" style={{
          fontSize: "11px",
          fontWeight: 500,
          color: isRunning ? "var(--warning)" : "var(--text-secondary)",
        }}>
          {call.name}
        </span>
        {isRunning ? (
          <span style={{ display: "flex", alignItems: "center", gap: "4px" }}>
            <Spinner size={10} />
            <span className="mono" style={{ fontSize: "10px", color: "var(--warning)" }}>running</span>
          </span>
        ) : (
          <span className="mono" style={{ fontSize: "10px", color: "var(--text-faint)" }}>
            completed
          </span>
        )}
      </button>
      {open && (
        <div style={{
          padding: "0 10px 10px",
          display: "flex",
          flexDirection: "column",
          gap: "8px",
        }}>
          {/* Args */}
          <div>
            <div className="mono" style={{
              fontSize: "9px",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              color: "var(--text-faint)",
              marginBottom: "4px",
            }}>
              Arguments
            </div>
            <pre style={{
              margin: 0,
              padding: "8px 10px",
              background: "var(--bg-0)",
              borderRadius: "var(--radius-sm)",
              border: "1px solid var(--border-faint)",
              fontSize: "11px",
              lineHeight: "1.5",
              fontFamily: "var(--font-mono)",
              color: "var(--text-secondary)",
              overflowX: "auto",
              whiteSpace: "pre-wrap",
            }}>
              {prettyArgs}
            </pre>
          </div>
          {/* Result */}
          {call.result && (
            <div>
              <div className="mono" style={{
                fontSize: "9px",
                textTransform: "uppercase",
                letterSpacing: "0.06em",
                color: "var(--text-faint)",
                marginBottom: "4px",
              }}>
                Result
              </div>
              <pre style={{
                margin: 0,
                padding: "8px 10px",
                background: "var(--bg-0)",
                borderRadius: "var(--radius-sm)",
                border: "1px solid var(--border-faint)",
                fontSize: "11px",
                lineHeight: "1.5",
                fontFamily: "var(--font-mono)",
                color: "var(--text-tertiary)",
                overflowX: "auto",
                whiteSpace: "pre-wrap",
                maxHeight: "300px",
                overflowY: "auto",
              }}>
                {call.result}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
