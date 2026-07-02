import React, { useEffect, useCallback, useState, useRef } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../components/Icon";
import { useSession, useMessages } from "../hooks/queries";
import { sendMessage, forkSession, onFrame } from "../api";
import type { Message, ServerFrame } from "../types";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

function fmtTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export default function ChatView({ sessionId }: { sessionId: string }) {
  const { data: session } = useSession(sessionId);
  const { data: msgData, isLoading } = useMessages(sessionId);
  const navigate = useNavigate();
  const [streamingText, setStreamingText] = useState("");
  const [sending, setSending] = useState(false);
  const [text, setText] = useState("");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const messages = msgData?.messages ?? [];

  // Listen for streaming deltas for this session
  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "message.delta" && frame.sessionId === sessionId) {
        setStreamingText((prev) => prev + frame.textDelta);
      }
      if (frame.kind === "message.done" && frame.sessionId === sessionId) {
        setStreamingText("");
        setSending(false);
      }
    });
    return unsub;
  }, [sessionId]);

  // Auto-scroll to bottom on new messages or streaming
  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [messages.length, streamingText]);

  const handleSend = useCallback(async () => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    setText("");
    try {
      await sendMessage(sessionId, trimmed);
    } catch {
      setText(trimmed); // restore on error
      setSending(false);
    }
  }, [text, sending, sessionId]);

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
    },
    [],
  );

  const handleFork = useCallback(async () => {
    try {
      const forked = await forkSession(sessionId);
      if (forked?.id) void navigate({ to: `/sessions/$sessionId`, params: { sessionId: forked.id } });
    } catch {
      // ignore — user can retry
    }
  }, [sessionId, navigate]);

  const isObserved = session?.managed === false;

  return (
    <div className="chatcol chat-view" data-session-id={sessionId}>
      <div className="vp-head">
        <div className="vp-left">
          <button
            type="button"
            className="icobtn"
            onClick={() => void navigate({ to: "/" })}
            title="Back"
          >
            <Icon name="chevron-left" />
          </button>
          <span className="vp-title chat-title">{session?.title || "Untitled"}</span>
          {session?.agent && <span className="gtag ok">{session.agent}</span>}
        </div>
        <div className="vp-right">
          {session?.liveness === "active" && (
            <div className="live chat-live-badge">
              <span className="dot" />
              <span className="lbl">LIVE</span>
            </div>
          )}
          {session?.managed && session?.liveness !== "active" && (
            <span className="gtag ok chat-managed-badge">managed</span>
          )}
        </div>
      </div>

      <div className="transcript" ref={transcriptRef}>
        <div className="tcol">
          {isLoading && <div className="msg-empty">Loading messages…</div>}
          {!isLoading && messages.length === 0 && !streamingText && (
            <div className="msg-empty">No messages yet. Send a message below.</div>
          )}
          {messages.map((m) => (
            <MessageBubble key={`${sessionId}-${m.messageId}`} msg={m} />
          ))}
          {streamingText && (
            <div className="msg-ai">
              <div className="who">{(session?.agent || "assistant").toUpperCase()}</div>
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{streamingText}</ReactMarkdown>
            </div>
          )}
        </div>
      </div>

      {isObserved ? (
        <div className="composer">
          <div className="obsbanner">
            <Icon name="alert" size={14} />
            <span>This is an observed session — read-only.</span>
            <button type="button" className="btn pri" onClick={handleFork}>
              Fork to continue
            </button>
          </div>
        </div>
      ) : (
        <div className="composer">
          <div className="comp-box">
            <textarea
              rows={1}
              className="composer-input"
              placeholder="Type a message…"
              value={text}
              onChange={handleTextareaInput}
              onKeyDown={handleKeyDown}
              autoFocus
            />
            <div className="comp-bar">
              <div className="comp-l">
                <button type="button" className="modelpill" title="Model">
                  <span className="dot" />
                  <span className="nm">{session?.model || "auto"}</span>
                </button>
              </div>
              <div className="comp-r">
                {sending && <span className="spin" />}
                <span className="comp-hint">↵ send · ⇧↵ newline</span>
                <button
                  type="button"
                  className="send"
                  onClick={handleSend}
                  disabled={!text.trim() || sending}
                  title="Send"
                >
                  <Icon name="arrow-up" size={14} />
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const MessageBubble = React.memo(function MessageBubble({ msg }: { msg: Message }) {
  const isUser = msg.role === "user";
  const ts = fmtTime(msg.timestamp);
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    navigator.clipboard?.writeText(msg.content || "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [msg.content]);

  return (
    <div className={isUser ? "msg-user" : "msg-ai"} data-ts={ts}>
      {!isUser && (
        <div className="who">{msg.role === "assistant" ? "ASSISTANT" : msg.role.toUpperCase()}</div>
      )}
      {isUser ? (
        msg.content
      ) : (
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content || ""}</ReactMarkdown>
      )}
      <div className="msg-acts">
        <button type="button" onClick={handleCopy} title="Copy">
          <Icon name={copied ? "check" : "copy"} size={12} />
        </button>
        <button type="button" title="Fork from here">
          <Icon name="git-branch" size={12} />
        </button>
        <span className="ts">{ts}</span>
      </div>
    </div>
  );
});
