import { useState, useEffect } from "react";
import { SessionList } from "./views/SessionList";
import { ChatView } from "./views/ChatView";
import { SearchView } from "./views/SearchView";

type View = "chat" | "search";

export default function App() {
  const [selectedSession, setSelectedSession] = useState<string | null>(null);
  const [view, setView] = useState<View>("chat");

  // Keyboard shortcut: Cmd/Ctrl+K → search
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setView("search");
      }
      if (e.key === "Escape") {
        setView("chat");
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const handleSelectSession = (id: string) => {
    setSelectedSession(id);
    setView("chat");
  };

  return (
    <div style={{
      display: "flex",
      width: "100%",
      height: "100%",
      overflow: "hidden",
    }}>
      {/* ── Sidebar ── */}
      <div style={{
        width: "var(--sidebar-w)",
        flexShrink: 0,
        height: "100%",
      }}>
        <SessionList
          selectedId={selectedSession}
          onSelect={handleSelectSession}
          onSearchClick={() => setView("search")}
        />
      </div>

      {/* ── Main panel ── */}
      <div style={{
        flex: 1,
        height: "100%",
        overflow: "hidden",
        position: "relative",
      }}>
        {view === "search" ? (
          <SearchView
            onOpenSession={handleSelectSession}
            onClose={() => setView("chat")}
          />
        ) : selectedSession ? (
          <ChatView sessionId={selectedSession} />
        ) : (
          <EmptyState />
        )}
      </div>
    </div>
  );
}

/* ── Empty state (no session selected) ── */
function EmptyState() {
  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      justifyContent: "center",
      height: "100%",
      gap: "12px",
    }}>
      <div style={{
        fontFamily: "var(--font-mono)",
        fontSize: "11px",
        color: "var(--text-faint)",
        letterSpacing: "0.15em",
        textTransform: "uppercase",
      }}>
        Olympus
      </div>
      <div style={{
        fontSize: "14px",
        color: "var(--text-tertiary)",
        maxWidth: "320px",
        textAlign: "center",
        lineHeight: "1.5",
      }}>
        Select a session from the list, or press{" "}
        <span className="mono" style={{
          display: "inline-block",
          padding: "1px 5px",
          borderRadius: "3px",
          border: "1px solid var(--border-subtle)",
          background: "var(--bg-2)",
          fontSize: "11px",
          color: "var(--text-secondary)",
        }}>
          ⌘K
        </span>
        {" "}to search.
      </div>
    </div>
  );
}
