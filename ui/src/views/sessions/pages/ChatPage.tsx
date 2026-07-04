/**
 * ChatPage — viewport content for the active session chat.
 *
 * Owns the transcript + composer ONLY (Page content). The View owns
 * the surrounding layout (right sidebar, bottom panel, header).
 *
 * Bug fixes:
 *  - Bug 7b: Optimistic message append — the user's message appears
 *    immediately before the server round-trip. Shows "thinking…" status
 *    via /ws (idle → thinking → streaming → done).
 *  - Bug 1: Fork confirmation uses the fixed-position ForkModal.
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Icon } from "../../../components/Icon";
import { useSession, useMessages } from "../../../hooks/queries";
import { sendMessage, forkSession, onFrame, respondPermission } from "../../../api";
import type { Message, ServerFrame } from "../../../types";
import { MessageBubble } from "../components/MessageBubble";
import { Composer } from "../components/Composer";
import { ForkModal } from "../components/ForkModal";

type AgentStatus = "idle" | "thinking" | "streaming" | "done";

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

  // streaming + status
  const [streamingText, setStreamingText] = useState("");
  const [sending, setSending] = useState(false);
  const [agentStatus, setAgentStatus] = useState<AgentStatus>("idle");
  const [text, setText] = useState("");
  const [optimisticMsg, setOptimisticMsg] = useState<Message | null>(null);
  // Pending permission request (ACP session/request_permission) for this session.
  const [permission, setPermission] = useState<{
    toolCall: string;
    options: Array<{ optionId: string; name: string; kind: string }>;
  } | null>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);

  const serverMessages = msgData?.messages ?? [];
  // Dedupe: once the server echoes the user's message (same role+content), drop
  // the optimistic copy so we don't show two identical bubbles while the agent
  // is thinking. (The optimistic id is -1; the real one has a server id.)
  const echoed =
    optimisticMsg != null &&
    serverMessages.some(
      (m) => m.role === "user" && m.content === optimisticMsg.content,
    );
  // Only show optimistic if no server messages with the same content exist.
  // The old check used `echoed` but a race could show both briefly.
  const hasServerEcho = serverMessages.some(
    (m) => m.role === "user" && m.content === optimisticMsg?.content && m.messageId >= 0,
  );
  const messages =
    optimisticMsg && !hasServerEcho ? [...serverMessages, optimisticMsg] : serverMessages;

  // Clear the optimistic copy as soon as the server echo lands.
  useEffect(() => {
    if (hasServerEcho) setOptimisticMsg(null);
  }, [hasServerEcho]);

  const isObserved = session?.managed === false;

  // Show a thinking indicator while the agent is working but hasn't streamed
  // text yet (thinking) — cleared once streaming text or the final reply lands.
  const showThinking =
    (agentStatus === "thinking" || sending) && !streamingText;

  // ── WS streaming + status (Bug 7b) ───────────────────────────────
  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      // Narrow by kind first — not all ServerFrame variants have sessionId.
      if (
        (frame.kind === "message.delta" || frame.kind === "message.done") &&
        frame.sessionId !== sessionId
      ) {
        return;
      }
      if (frame.kind === "message.delta") {
        setStreamingText((prev) => prev + frame.textDelta);
        setAgentStatus("streaming");
      }
      if (frame.kind === "message.done") {
        setStreamingText("");
        setSending(false);
        setAgentStatus("done");
        // Clear optimistic message once the server message arrives
        setOptimisticMsg(null);
        setPermission(null);
      }
      // Agent is blocked awaiting a permission decision for a gated tool call.
      if (frame.kind === "permission.required" && frame.sessionId === sessionId) {
        setPermission({ toolCall: frame.toolCall, options: frame.options });
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
  }, [messages.length, streamingText, agentStatus]);

  // ── Send (Bug 7b: optimistic + thinking status) ──────────────────
  const handleSend = useCallback(async (model?: string) => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;

    setSending(true);
    setAgentStatus("thinking");
    setText("");

    // Optimistic: append the user message immediately
    const now = Math.floor(Date.now() / 1000);
    const optimistic: Message = {
      messageId: -1,
      sessionId,
      role: "user",
      content: trimmed,
      toolName: null,
      toolCalls: null,
      reasoning: null,
      timestamp: now,
      tokenCount: null,
      finishReason: null,
    };
    setOptimisticMsg(optimistic);

    try {
      await sendMessage(sessionId, trimmed, model);
      // The server returns 202; the agent status stays "thinking" until
      // the first /ws delta arrives (handled by the effect above).
    } catch {
      setText(trimmed); // restore on error
      setSending(false);
      setAgentStatus("idle");
      setOptimisticMsg(null);
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

  // ── Fork (Bug 1: fixed-position modal) ───────────────────────────
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
            {!isLoading && messages.length === 0 && !streamingText && (
              <div className="msg-empty">
                No messages yet. Send a message below.
              </div>
            )}
            {messages.map((m) => (
              <MessageBubble
                key={`${sessionId}-${m.messageId}`}
                msg={m}
                onFork={handleForkRequest}
              />
            ))}
            {/* Streaming assistant reply */}
            {streamingText && (
              <div className="msg-ai">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {streamingText}
                </ReactMarkdown>
              </div>
            )}
            {/* Thinking indicator — agent is working, no text streamed yet */}
            {showThinking && (
              <div className="msg-ai thinking-row">
                <span className="thinking-dots" aria-label="thinking">
                  <i /><i /><i />
                </span>
                <span className="thinking-label">thinking…</span>
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
        <Composer
          text={text}
          onTextChange={handleTextareaInput}
          onKeyDown={handleKeyDown}
          onSend={handleSend}
          sending={sending}
          sessionModel={session?.model ?? null}
          sessionAgent={session?.agent ?? null}
          sessionNode={session?.node ?? null}
        />
      )}

      {/* Bug 1: fixed-position fork modal */}
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
