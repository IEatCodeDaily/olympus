import { useState, useEffect, useCallback, useRef } from "react";
import type { Message } from "../types";
import { fetchMessages, onFrame, sendFrame, connectWs } from "../api";

export interface UseChatResult {
  messages: Message[];
  loading: boolean;
  error: string | null;
  streamingText: Record<number, string>; // messageId → accumulating text
  streamingIds: Set<number>;
  loadOlder: () => void;
  hasOlder: boolean;
}

export function useChat(sessionId: string | null): UseChatResult {
  const [messages, setMessages] = useState<Message[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [streamingText, setStreamingText] = useState<Record<number, string>>({});
  const [hasOlder, setHasOlder] = useState(false);
  const currentSession = useRef<string | null>(null);

  // Load messages when session changes
  useEffect(() => {
    if (!sessionId) {
      setMessages([]);
      return;
    }
    currentSession.current = sessionId;
    setLoading(true);
    setError(null);

    fetchMessages(sessionId, { limit: 50 })
      .then((res) => {
        if (currentSession.current !== sessionId) return;
        setMessages(res.messages);
        setHasOlder(res.nextCursor !== null);
      })
      .catch((e) => {
        if (currentSession.current !== sessionId) return;
        setError(String(e));
      })
      .finally(() => {
        if (currentSession.current !== sessionId) return;
        setLoading(false);
      });
  }, [sessionId]);

  // Subscribe to this session's message stream
  useEffect(() => {
    if (!sessionId) return;
    // Ensure WS is connected
    connectWs();
    sendFrame({ kind: "subscribe", sessionId });

    return () => {
      sendFrame({ kind: "unsubscribe", sessionId });
      setStreamingText({});
    };
  }, [sessionId]);

  // Listen for delta/done frames for this session
  useEffect(() => {
    if (!sessionId) return;
    return onFrame((frame) => {
      if (frame.kind === "message.delta" && frame.sessionId === sessionId) {
        setStreamingText((prev) => ({
          ...prev,
          [frame.messageId]: (prev[frame.messageId] ?? "") + frame.textDelta,
        }));
      } else if (frame.kind === "message.done" && frame.sessionId === sessionId) {
        // The final message already exists in the list; clear streaming text
        setTimeout(() => {
          setStreamingText((prev) => {
            const next = { ...prev };
            delete next[frame.messageId];
            return next;
          });
        }, 500);
      } else if (frame.kind === "message.appended" && frame.sessionId === sessionId) {
        setMessages((prev) => {
          if (prev.some((m) => m.messageId === frame.message.messageId)) return prev;
          return [...prev, frame.message];
        });
      }
    });
  }, [sessionId]);

  const streamingIds = new Set(Object.keys(streamingText).map(Number));

  const loadOlder = useCallback(async () => {
    if (!sessionId || messages.length === 0) return;
    try {
      const res = await fetchMessages(sessionId, { limit: 50 });
      // In mock mode there's no real cursor; prepend if we got different messages
      const existingIds = new Set(messages.map((m) => m.messageId));
      const older = res.messages.filter((m) => !existingIds.has(m.messageId));
      if (older.length > 0) {
        setMessages((prev) => [...older, ...prev]);
      }
      setHasOlder(res.nextCursor !== null);
    } catch {
      setHasOlder(false);
    }
  }, [sessionId, messages]);

  return {
    messages,
    loading,
    error,
    streamingText,
    streamingIds,
    loadOlder,
    hasOlder,
  };
}
