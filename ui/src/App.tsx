import { useState, useEffect } from "react";
import SessionList from "./views/SessionList";
import ChatView from "./views/ChatView";
import SearchView from "./views/SearchView";
import BoardView from "./views/BoardView";
import NodesView from "./views/NodesView";
import WorkflowsView from "./views/WorkflowsView";
import UsageView from "./views/UsageView";
import SettingsView from "./views/SettingsView";
import { healthCheck, connectWs, onFrame } from "./api";
import { useTheme, THEMES, THEME_LABELS } from "./lib/theme";
import type { HealthResponse } from "./types";

type ViewName =
  | "sessions"
  | "search"
  | "board"
  | "nodes"
  | "workflows"
  | "usage"
  | "settings";

interface NavDef {
  name: ViewName;
  label: string;
  icon: JSX.Element;
}

const ICON = (d: string, extra?: JSX.Element) => (
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <path d={d} />
    {extra}
  </svg>
);

const NAV: NavDef[] = [
  { name: "sessions", label: "Sessions", icon: ICON("M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01") },
  { name: "search", label: "Search", icon: <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.35-4.35" /></svg> },
  { name: "board", label: "Board", icon: <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="6" height="18" rx="1" /><rect x="9" y="3" width="6" height="12" rx="1" /><rect x="15" y="3" width="6" height="9" rx="1" /></svg> },
  { name: "nodes", label: "Nodes", icon: <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="2" y="2" width="8" height="8" rx="1" /><rect x="14" y="2" width="8" height="8" rx="1" /><rect x="8" y="14" width="8" height="8" rx="1" /><path d="M6 10v2a2 2 0 0 0 2 2h0M18 10v2a2 2 0 0 1-2 2h0" /></svg> },
  { name: "workflows", label: "Workflows", icon: <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="6" cy="6" r="3" /><circle cx="18" cy="18" r="3" /><path d="M9 6h6a3 3 0 0 1 3 3v6" /></svg> },
  { name: "usage", label: "Usage", icon: ICON("M3 3v18h18M7 16l4-6 3 3 5-7") },
  { name: "settings", label: "Settings", icon: <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg> },
];

export default function App() {
  const [view, setView] = useState<ViewName>("sessions");
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [syncConnected, setSyncConnected] = useState(true);
  const [theme, setTheme] = useTheme();

  useEffect(() => {
    connectWs();
    healthCheck().then(setHealth).catch(() => {});
    return onFrame((frame) => {
      if (frame.kind === "sync.status") setSyncConnected(frame.connected);
      if (frame.kind === "hello") setSyncConnected(true);
    });
  }, []);

  const openSession = (id: string) => {
    setView("sessions");
    setSelectedSessionId(id);
  };
  const backToList = () => setSelectedSessionId(null);

  const cycleTheme = () => {
    const i = THEMES.indexOf(theme);
    setTheme(THEMES[(i + 1) % THEMES.length]);
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M12 2 2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
            </svg>
          </div>
          <div className="brand-text">
            <span className="brand-name">Olympus</span>
            <span className="brand-sub">control plane</span>
          </div>
        </div>

        <nav className="nav">
          {NAV.map((item) => (
            <button
              key={item.name}
              className={`nav-item ${view === item.name ? "active" : ""}`}
              onClick={() => {
                if (item.name === "sessions") backToList();
                setView(item.name);
              }}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </nav>

        <div className="status-panel">
          <div className="status-row">
            <span className={`status-dot ${syncConnected ? "connected" : "disconnected"}`} />
            <span className="status-label">{syncConnected ? "synced" : "disconnected"}</span>
          </div>
          {health && (
            <>
              <div className="status-row">
                <span className="status-key">profile</span>
                <span className="status-val">{health.hermesProfile}</span>
              </div>
              {health.snapshot && (
                <div className="status-row">
                  <span className="status-key">store</span>
                  <span className="status-val">{health.snapshot.sessions} sess / {health.snapshot.messages} msg</span>
                </div>
              )}
              <div className="status-row">
                <span className="status-key">import</span>
                <span className="status-val">{health.importState}</span>
              </div>
            </>
          )}
          <button className="theme-toggle" onClick={cycleTheme} title="Switch theme">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="5" /><path d="M12 1v2M12 21v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1 12h2M21 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4" /></svg>
            {THEME_LABELS[theme]}
          </button>
        </div>
      </aside>

      <main className="main">
        {view === "sessions" && (
          <>
            <div className="list-pane">
              <SessionList selectedId={selectedSessionId} onOpenSession={openSession} />
            </div>
            {selectedSessionId && (
              <div className="chat-pane">
                <ChatView sessionId={selectedSessionId} onBack={backToList} onOpenSession={openSession} />
              </div>
            )}
          </>
        )}
        {view === "search" && <SearchView onOpenSession={openSession} />}
        {view === "board" && <BoardView />}
        {view === "nodes" && <NodesView />}
        {view === "workflows" && <WorkflowsView />}
        {view === "usage" && <UsageView />}
        {view === "settings" && <SettingsView />}
      </main>
    </div>
  );
}
