// Fake WebSocket that speaks the ServerFrame/ClientFrame protocol from the contract.
// Intercepts `new WebSocket(ws://…/ws)` in dev so the UI's WS client works with zero backend.
//
// Protocol implemented (docs/api-contract.md §WSS):
//   On connect: { kind:"hello", snapshot }
//   Then: periodic session.added / session.updated frames to simulate live traffic
//   On subscribe {sessionId}: replays message.delta tokens then message.done
//
// When mocks are disabled (VITE_USE_MOCKS=false), the real api.ts WebSocket path runs.

import { SESSIONS, MESSAGES_BY_SESSION } from "./fixtures";
import type { ServerFrame, ClientFrame } from "../types";

type MsgListener = (e: { data: string }) => void;
type CloseListener = () => void;

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  readonly readyState = 1; // OPEN
  onmessage: MsgListener | null = null;
  onclose: CloseListener | null = null;
  onerror: ((e: unknown) => void) | null = null;
  onopen: (() => void) | null = null;

  private subscriptions = new Set<string>();
  private timers: ReturnType<typeof setTimeout>[] = [];
  private sessionTimer: ReturnType<typeof setInterval> | null = null;
  private liveSessionIdx = 0;

  constructor(public url: string) {
    MockWebSocket.instances.push(this);
    // Fire onopen on next tick (mimics real WS)
    setTimeout(() => {
      this.onopen?.();
      this.sendHello();
      this.startLiveSessionTraffic();
    }, 50);
  }

  send(data: string): void {
    let frame: ClientFrame;
    try {
      frame = JSON.parse(data) as ClientFrame;
    } catch {
      return;
    }
    if (frame.kind === "subscribe") {
      this.subscriptions.add(frame.sessionId);
      this.startMessageStream(frame.sessionId);
    } else if (frame.kind === "unsubscribe") {
      this.subscriptions.delete(frame.sessionId);
    }
  }

  close(): void {
    this.subscriptions.clear();
    this.timers.forEach(clearTimeout);
    if (this.sessionTimer) clearInterval(this.sessionTimer);
    this.onclose?.();
  }

  private emit(frame: ServerFrame): void {
    this.onmessage?.({ data: JSON.stringify(frame) });
  }

  private sendHello(): void {
    const totalMsgs = Object.values(MESSAGES_BY_SESSION).reduce(
      (s, arr) => s + arr.length,
      0
    );
    this.emit({
      kind: "hello",
      snapshot: { sessions: SESSIONS.length, messages: totalMsgs },
    });
  }

  // Simulate live session.* frames every ~15s — new sessions appear, existing update
  private startLiveSessionTraffic(): void {
    this.sessionTimer = setInterval(() => {
      // Occasionally bump lastActivity + messageCount on a random session
      if (Math.random() < 0.6) {
        const sess = SESSIONS[Math.floor(Math.random() * SESSIONS.length)];
        this.emit({
          kind: "session.updated",
          sessionId: sess.id,
          changes: {
            lastActivity: Math.floor(Date.now() / 1000),
            messageCount: sess.messageCount + 1,
          },
        });
      }
    }, 15000);
  }

  // On subscribe: replay message.delta tokens for the latest assistant message, then message.done
  private startMessageStream(sessionId: string): void {
    const msgs = MESSAGES_BY_SESSION[sessionId];
    if (!msgs || msgs.length === 0) return;

    // Pick the last assistant message to "stream"
    const lastAssistant = [...msgs].reverse().find((m) => m.role === "assistant");
    if (!lastAssistant?.content) return;

    // Short delay before first token
    const baseDelay = 300;
    const words = lastAssistant.content.split(" ");
    let accumulated = "";

    // Stream word groups as deltas
    words.forEach((word, i) => {
      const delay = baseDelay + i * 40 + Math.random() * 30;
      const t = setTimeout(() => {
        accumulated += (i > 0 ? " " : "") + word;
        // Only stream if still subscribed
        if (!this.subscriptions.has(sessionId)) return;
        this.emit({
          kind: "message.delta",
          sessionId,
          messageId: lastAssistant.messageId,
          textDelta: (i > 0 ? " " : "") + word,
        });
        // Last word → message.done
        if (i === words.length - 1) {
          const td = setTimeout(() => {
            if (!this.subscriptions.has(sessionId)) return;
            this.emit({
              kind: "message.done",
              sessionId,
              messageId: lastAssistant.messageId,
              finishReason: "stop",
            });
          }, 150);
          this.timers.push(td);
        }
      }, delay);
      this.timers.push(t);
    });
  }
}

let installed = false;

export function installWsMock(): void {
  if (installed || typeof window === "undefined") return;
  if (import.meta.env.VITE_USE_MOCKS === "false") return;
  installed = true;
  // @ts-expect-error — deliberate override for mock mode
  window.WebSocket = MockWebSocket;
}
