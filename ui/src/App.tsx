import React, { useEffect, useCallback, useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Icon, type IconName } from "./components/Icon";
import { useUIStore, type ViewName } from "./store";
import {
  useSessions,
  useSession,
  useMessages,
  useHealth,
  useLiveSync,
} from "./hooks/queries";
import { sendMessage } from "./api";
import { onFrame } from "./api";
import type { Message, ServerFrame, HealthResponse, Session } from "./types";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// ── Query client ──────────────────────────────────
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

// ── Layout chips config ───────────────────────────
const LAYOUTS: { sec: ViewName; label: string; icon: IconName }[] = [
  { sec: "sessions", label: "Sessions", icon: "message-square" },
  { sec: "history", label: "History", icon: "archive" },
  { sec: "board", label: "Board", icon: "kanban" },
  { sec: "nodes", label: "Fleet", icon: "server" },
  { sec: "workflows", label: "Workflow", icon: "workflow" },
  { sec: "plugins", label: "Plugins", icon: "puzzle" },
  { sec: "settings", label: "Settings", icon: "gear" },
];

// ── Time format helper ────────────────────────────
function timeAgo(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

// ═══════════════════════════════════════════════════
// App Root
// ═══════════════════════════════════════════════════
export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppInner />
    </QueryClientProvider>
  );
}

function AppInner() {
  const { view } = useUIStore();
  useLiveSync();

  return (
    <div className="app">
      <TopBar />
      <div className="body">
        <LeftSidebar />
        <MainViewport />
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════════
// TopBar
// ═══════════════════════════════════════════════════
function TopBar() {
  const { view, setView, toggleSidebar, setPaletteOpen } = useUIStore();
  const { data: health } = useHealth() as { data: HealthResponse | undefined };

  return (
    <div className="topbar">
      <div className="tb-left">
        <button type="button" className="icobtn" onClick={toggleSidebar} title="Toggle sidebar">
          <Icon name="panel-left" />
        </button>
        <div className="divider" />
        <div className="layouts">
          {LAYOUTS.map((l) => (
            <button
              type="button"
              key={l.sec}
              className={`chip ${view === l.sec ? "on" : ""}`}
              onClick={() => setView(l.sec)}
            >
              <Icon name={l.icon} size={12} />
              {l.label}
            </button>
          ))}
        </div>
      </div>
      <div className="tb-center">
        <button type="button" className="tb-search" onClick={() => setPaletteOpen(true)}>
          <Icon name="search" size={13} />
          <span className="ph">Search sessions, messages…</span>
          <span className="sp" />
          <span className="kbd">⌘K</span>
        </button>
      </div>
      <div className="tb-right">
        <div className="org">
          <div className="mk" />
          <span className="nm">{health?.hermesProfile ?? "default"}</span>
        </div>
        <div className="profile">rp</div>
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════════
// Left Sidebar
// ═══════════════════════════════════════════════════
function LeftSidebar() {
  const {
    sidebarCollapsed,
    sidebarWidth,
    view,
    setView,
    setActiveSession,
  } = useUIStore();

  const { data: sessionData } = useSessions({ managed: true });
  const { data: historyData } = useSessions({ managed: false, limit: 20 });

  const sessions = sessionData?.sessions ?? [];
  const history = historyData?.sessions ?? [];

  const handleNewSession = useCallback(async () => {
    // Create a session via the API, then select it.
    try {
      const res = await fetch("/api/sessions", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${import.meta.env.VITE_API_TOKEN}`,
        },
        body: JSON.stringify({ title: null, model: null }),
      });
      if (res.ok) {
        const data = await res.json();
        setActiveSession(data.session?.id ?? null);
        setView("sessions");
      }
    } catch {
      // ignore — the sessions list will refetch
    }
  }, [setActiveSession, setView]);

  if (sidebarCollapsed) return null;

  return (
    <>
      <div className="sidebar" style={{ width: sidebarWidth }}>
        <div className="sb-pad">
          <button type="button" className="newbtn" onClick={handleNewSession}>
            <Icon name="plus" size={14} />
            New Session
          </button>
        </div>
        <div className="sb-scroll">
          {view === "sessions" || view === "history" ? (
            <>
              <SessionSection
                label="ACTIVE"
                sessions={sessions}
                count={sessions.length}
              />
              {history.length > 0 && (
                <SessionSection
                  label="HISTORY"
                  sessions={history}
                  count={history.length}
                  dimmed
                />
              )}
            </>
          ) : (
            <div className="sb-pad">
              <button
                type="button"
                className={`navitem on`}
                onClick={() => setView("sessions")}
              >
                <Icon name="message-square" size={14} />
                Sessions
              </button>
              <button
                type="button"
                className={`navitem ${view === "board" ? "on" : ""}`}
                onClick={() => setView("board")}
              >
                <Icon name="kanban" size={14} />
                Board
              </button>
              <button
                type="button"
                className={`navitem ${view === "nodes" ? "on" : ""}`}
                onClick={() => setView("nodes")}
              >
                <Icon name="server" size={14} />
                Fleet
              </button>
            </div>
          )}
        </div>
      </div>
      <div className="rz-x" />
    </>
  );
}

function SessionSection({
  label,
  sessions,
  count,
  dimmed,
}: {
  label: string;
  sessions: Session[];
  count: number;
  dimmed?: boolean;
}) {
  const { activeSessionId, setActiveSession, setView } = useUIStore();
  if (count === 0) return null;

  return (
    <>
      <div className="sec-head">
        <span className="lbl">{label}</span>
        <span className="sp" />
        <span className="ct">{count}</span>
      </div>
      <div className="sec-content">
        {sessions.slice(0, 50).map((s) => (
          <button
            type="button"
            key={s.id}
            className={`srow ${activeSessionId === s.id ? "on" : ""}`}
            onClick={() => {
              setActiveSession(s.id);
              setView("sessions");
            }}
          >
            <span className={`dot ${s.liveness === "active" ? "active" : "idle"}`} />
            <span className="info">
              <span className="title">{s.title || "Untitled"}</span>
            </span>
            <span className="meta">
              <span>{s.messageCount}</span>
              <span>{timeAgo(s.lastActivity)}</span>
            </span>
          </button>
        ))}
      </div>
    </>
  );
}

// ═══════════════════════════════════════════════════
// Main Viewport
// ═══════════════════════════════════════════════════
function MainViewport() {
  const { view } = useUIStore();

  return (
    <div className="viewport">
      {view === "sessions" ? (
        <ChatViewport />
      ) : (
        <GenericView title={LAYOUTS.find((l) => l.sec === view)?.label ?? view} />
      )}
    </div>
  );
}

function GenericView({ title }: { title: string }) {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">{title}</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <div className="empty-state-title">{title} view</div>
          <div className="empty-state-msg">
            This view is part of the Olympus control plane. Backend wiring in progress.
          </div>
        </div>
      </div>
    </>
  );
}

// ═══════════════════════════════════════════════════
// Chat Viewport (transcript + composer + right sidebar)
// ═══════════════════════════════════════════════════
function ChatViewport() {
  const { activeSessionId, bottomCollapsed, toggleBottom, rightSidebarCollapsed, toggleRightSidebar } =
    useUIStore();
  const { data: session } = useSession(activeSessionId);

  if (!activeSessionId) {
    return (
      <>
        <div className="vp-head">
          <div className="vp-left">
            <span className="vp-title">No session selected</span>
          </div>
        </div>
        <div className="vp-body">
          <div className="empty-state">
            <Icon name="message-square" size={32} />
            <div className="empty-state-title">Select a session</div>
            <div className="empty-state-msg">
              Choose a session from the sidebar or create a new one to start chatting.
            </div>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="vp-head">
        <div className="vp-left">
          <span className="vp-title">{session?.title || "Untitled session"}</span>
          {session?.agent && (
            <span className="gtag ok">{session.agent}</span>
          )}
        </div>
        <div className="vp-right">
          {session?.liveness === "active" && (
            <div className="live">
              <span className="dot" />
              <span className="lbl">LIVE</span>
            </div>
          )}
          <button type="button" className="toggle" onClick={toggleBottom} title="Toggle bottom panel">
            <Icon name="panel-bottom" size={14} />
          </button>
          <button type="button" className="toggle" onClick={toggleRightSidebar} title="Toggle right panel">
            <Icon name="panel-right" size={14} />
          </button>
        </div>
      </div>
      <div className="vp-body">
        <ChatColumn sessionId={activeSessionId} />
        {!rightSidebarCollapsed && <RightSidebar sessionId={activeSessionId} />}
      </div>
      {!bottomCollapsed && <BottomPanel />}
    </>
  );
}

// ═══════════════════════════════════════════════════
// Chat Column (transcript + composer)
// ═══════════════════════════════════════════════════
function ChatColumn({ sessionId }: { sessionId: string }) {
  const { data: msgData } = useMessages(sessionId);
  const [streamingText, setStreamingText] = useState("");
  const messages = msgData?.messages ?? [];

  // Listen for streaming deltas for this session.
  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "message.delta" && frame.sessionId === sessionId) {
        setStreamingText((prev) => prev + frame.textDelta);
      }
      if (frame.kind === "message.done" && frame.sessionId === sessionId) {
        setStreamingText("");
      }
    });
    return unsub;
  }, [sessionId]);

  return (
    <div className="chatcol">
      <div className="transcript">
        <div className="tcol">
          {messages.length === 0 && !streamingText && (
            <div className="msg-empty">No messages yet. Send a message below.</div>
          )}
          {messages.map((m) => (
            <MessageBubble key={m.messageId} msg={m} />
          ))}
          {streamingText && (
            <div className="msg-ai">
              <div className="who">ASSISTANT</div>
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{streamingText}</ReactMarkdown>
            </div>
          )}
        </div>
      </div>
      <Composer sessionId={sessionId} />
    </div>
  );
}

const MessageBubble = React.memo(function MessageBubble({ msg }: { msg: Message }) {
  const isUser = msg.role === "user";
  const ts = new Date(msg.timestamp * 1000).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  return (
    <div className={isUser ? "msg-user" : "msg-ai"}>
      {!isUser && <div className="who">ASSISTANT</div>}
      {isUser ? (
        msg.content
      ) : (
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content || ""}</ReactMarkdown>
      )}
    </div>
  );
});

// ═══════════════════════════════════════════════════
// Composer
// ═══════════════════════════════════════════════════
function Composer({ sessionId }: { sessionId: string }) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const { data: session } = useSession(sessionId);

  const handleSend = useCallback(async () => {
    if (!text.trim() || sending) return;
    setSending(true);
    try {
      await sendMessage(sessionId, text.trim());
      setText("");
    } catch {
      // keep text on error so user can retry
    } finally {
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

  return (
    <div className="composer">
      <div className="comp-box">
        <textarea
          rows={1}
          placeholder="Send a message…"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          autoFocus
        />
        <div className="comp-bar">
          <div className="comp-l">
            <button type="button" className="modelpill">
              <span className="dot" />
              <span className="nm">{session?.model || "auto"}</span>
            </button>
          </div>
          <div className="comp-r">
            <span className="comp-hint">↵ to send · ⇧↵ for newline</span>
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
  );
}

// ═══════════════════════════════════════════════════
// Right Sidebar
// ═══════════════════════════════════════════════════
function RightSidebar({ sessionId }: { sessionId: string }) {
  const { rightTab, setRightTab } = useUIStore();
  const { data: session } = useSession(sessionId);
  const { data: msgData } = useMessages(sessionId);

  return (
    <div className="rsidebar">
      <div className="rs-tabbar">
        <button
          type="button"
          className={`rs-tab ${rightTab === "info" ? "on" : ""}`}
          onClick={() => setRightTab("info")}
          title="Info"
        >
          <Icon name="activity" size={13} />
        </button>
        <button
          type="button"
          className={`rs-tab ${rightTab === "artifacts" ? "on" : ""}`}
          onClick={() => setRightTab("artifacts")}
          title="Artifacts"
        >
          <Icon name="file" size={13} />
        </button>
      </div>
      {rightTab === "info" && (
        <>
          <div className="rs-sec">
            <div className="kv">
              <span className="k">SESSION</span>
              <span className="v">{sessionId.slice(0, 12)}</span>
            </div>
            <div className="kv">
              <span className="k">MODEL</span>
              <span className="v">{session?.model || "—"}</span>
            </div>
            <div className="kv">
              <span className="k">AGENT</span>
              <span className="v">{session?.agent || "—"}</span>
            </div>
            <div className="kv">
              <span className="k">SOURCE</span>
              <span className="v">{session?.source || "—"}</span>
            </div>
            <div className="kv">
              <span className="k">MESSAGES</span>
              <span className="v">{session?.messageCount ?? 0}</span>
            </div>
            <div className="kv">
              <span className="k">TOKENS</span>
              <span className="v">
                {((session?.inputTokens ?? 0) + (session?.outputTokens ?? 0)).toLocaleString()}
              </span>
            </div>
          </div>
          <div className="rs-sec">
            <div className="rs-label">STATS</div>
            <div className="stats">
              <div className="stat">
                <span className="v">{session?.inputTokens ?? 0}</span>
                <span className="l">IN</span>
              </div>
              <div className="stat">
                <span className="v">{session?.outputTokens ?? 0}</span>
                <span className="l">OUT</span>
              </div>
            </div>
          </div>
        </>
      )}
      {rightTab === "artifacts" && (
        <div className="rs-sec">
          <div className="rs-label">ARTIFACTS</div>
          <div className="empty-state-msg">No artifacts generated yet.</div>
        </div>
      )}
    </div>
  );
}

// ═══════════════════════════════════════════════════
// Bottom Panel (events tail)
// ═══════════════════════════════════════════════════
function BottomPanel() {
  const { bottomTab, setBottomTab, toggleBottom } = useUIStore();
  const [events, setEvents] = useState<string[]>([]);

  useEffect(() => {
    // Tail the event log endpoint
    let since = 0;
    const interval = setInterval(async () => {
      try {
        const res = await fetch(
          `/api/events?since=${since}&limit=20`,
          { headers: { Authorization: `Bearer ${import.meta.env.VITE_API_TOKEN}` } },
        );
        if (res.ok) {
          const data = await res.json();
          if (data.events?.length > 0) {
            since = data.events[data.events.length - 1].seq + 1;
            setEvents((prev) =>
              [...prev, ...data.events.map((e: { event: string }) => JSON.stringify(e.event).slice(0, 200))].slice(-100),
            );
          }
        }
      } catch {
        // ignore
      }
    }, 3000);
    return () => clearInterval(interval);
  }, []);

  return (
    <div className="bpanel">
      <div className="bp-tabs">
        <div className="bp-tablist">
          <button
            type="button"
            className={`bp-tab ${bottomTab === "events" ? "on" : ""}`}
            onClick={() => setBottomTab("events")}
          >
            Events
          </button>
          <button
            type="button"
            className={`bp-tab ${bottomTab === "logs" ? "on" : ""}`}
            onClick={() => setBottomTab("logs")}
          >
            Logs
          </button>
        </div>
        <div className="bp-right">
          <button type="button" className="toggle" onClick={toggleBottom}>
            <Icon name="chevron-down" size={14} />
          </button>
        </div>
      </div>
      <div className="bp-body">
        {events.length === 0 ? (
          <div className="ln d">No events yet.</div>
        ) : (
          events.map((e, i) => (
            <div key={i} className="ln d">
              {e}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
