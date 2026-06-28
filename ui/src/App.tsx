import { useState, useEffect } from "react";
import SessionList from "./views/SessionList";
import ChatView from "./views/ChatView";
import SearchView from "./views/SearchView";
import { healthCheck } from "./api";
import { connectWs, onFrame } from "./api";
import type { HealthResponse } from "./types";

type View = { name: "sessions" } | { name: "search" };

export default function App() {
  // In sessions view: selectedSessionId drives the right panel (master-detail).
  // null = nothing selected (list-only). Search view remains full-swap.
  const [view, setView] = useState<View>({ name: "sessions" });
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [syncConnected, setSyncConnected] = useState(true);

  useEffect(() => {
    connectWs();
    healthCheck().then(setHealth).catch(() => {});
    return onFrame((frame) => {
      if (frame.kind === "sync.status") setSyncConnected(frame.connected);
      if (frame.kind === "hello") setSyncConnected(true);
    });
  }, []);

  const openSession = (id: string) => {
    // Always switch to sessions view + set the selection (works from Search too)
    setView({ name: "sessions" });
    setSelectedSessionId(id);
  };
  const backToList = () => setSelectedSessionId(null);

  return (
    <div className="app">
      {/* Sidebar */}
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
          <button
            className={`nav-item ${view.name === "sessions" ? "active" : ""}`}
            onClick={backToList}
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="8" y1="6" x2="21" y2="6" /><line x1="8" y1="12" x2="21" y2="12" /><line x1="8" y1="18" x2="21" y2="18" />
              <line x1="3" y1="6" x2="3.01" y2="6" /><line x1="3" y1="12" x2="3.01" y2="12" /><line x1="3" y1="18" x2="3.01" y2="18" />
            </svg>
            Sessions
          </button>
          <button
            className={`nav-item ${view.name === "search" ? "active" : ""}`}
            onClick={() => setView({ name: "search" })}
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="8" /><path d="m21 21-4.35-4.35" />
            </svg>
            Search
          </button>
        </nav>

        {/* Status panel */}
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
        </div>
      </aside>

      {/* Main content — master-detail for sessions, full-swap for search */}
      <main className="main">
        {view.name === "sessions" && (
          <>
            <div className="list-pane">
              <SessionList
                selectedId={selectedSessionId}
                onOpenSession={openSession}
              />
            </div>
            {selectedSessionId && (
              <div className="chat-pane">
                <ChatView sessionId={selectedSessionId} onBack={backToList} />
              </div>
            )}
          </>
        )}
        {view.name === "search" && <SearchView onOpenSession={openSession} />}
      </main>
    </div>
  );
}
