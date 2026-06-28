import { useRef, useEffect, useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { useChat } from "../hooks/useChat";
import { useSessions } from "../hooks/useSessions";
import { formatTime, SOURCE_META, formatTokens } from "../lib/format";
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

  return (
    <div className="chat-view">
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
        </div>
        <div className="chat-stats">
          {session && <span>{session.messageCount} msgs · {formatTokens(session.inputTokens + session.outputTokens)} tok</span>}
        </div>
      </div>

      {/* Messages */}
      <div className="chat-scroll" ref={scrollRef} onScroll={handleScroll}>
        {loading && <div className="chat-loading">Loading conversation...</div>}
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
    </div>
  );
}

function MessageBubble({ message, streamingText }: { message: Message; streamingText?: string }) {
  const isUser = message.role === "user";
  const isAssistant = message.role === "assistant";
  const isTool = message.role === "tool";
  const isSystem = message.role === "system";
  const [showReasoning, setShowReasoning] = useState(false);

  const content = streamingText ?? message.content;

  if (isSystem) {
    return (
      <div className="msg msg-system">
        <span className="msg-system-text">{content}</span>
      </div>
    );
  }

  return (
    <div className={`msg msg-${message.role}`}>
      <div className="msg-role">
        {isUser ? "you" : isAssistant ? "ai" : isTool ? "tool" : "?"}
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
        {message.tokenCount && (
          <div className="msg-meta">{message.tokenCount} tokens · {formatTime(message.timestamp)}</div>
        )}
        {streamingText !== undefined && (
          <div className="msg-meta streaming">streaming...</div>
        )}
      </div>
    </div>
  );
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
