/**
 * SessionsView — the Sessions View component (owns sidebar + viewport layout).
 *
 * Architecture (per docs/plans/2026-07-03-olympus-usable-5-surfaces.md):
 *
 * The View OWNS:
 *   - left sidebar (session list + NavItems) — SessionSidebar
 *   - viewport LAYOUT (center chat + right sidebar + bottom panel)
 *   - right sidebar content — RightPanel
 *   - bottom panel content — BottomPanel
 *
 * Pages own viewport content ONLY:
 *   - ChatPage (the transcript + composer)
 *   - AgentsPage
 *   - UsagePage
 *
 * Routes (URL-persistent):
 *   /sessions          → empty pane (no session selected)
 *   /sessions/$id      → ChatPage
 *   /sessions/agents   → AgentsPage
 *   /sessions/usage    → UsagePage
 *
 *   ┌──────────────────────────────────────────────────────────────┐
 *   │ vp-head (title · project badge · live badge · panel toggles) │
 *   ├──────────────────────────────────────────────────────────────┤
 *   │ vp-body                                                      │
 *   │   chatcol (flex:1)              │ rz-x │ rsidebar            │
 *   │     transcript                  │      │                      │
 *   │     rz-y                        │      │                      │
 *   │     bpanel                      │      │                      │
 *   ├──────────────────────────────────────────────────────────────┤
 *   │ composer (ChatPage-owned, rendered inside chatcol)           │
 *   └──────────────────────────────────────────────────────────────┘
 */

import React, { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../components/Icon";
import { BrandIcon, agentBrand } from "../components/BrandIcons";
import { useUIStore } from "../store";
import { useSession, useMessages, useAgents } from "../hooks/queries";
import { useResizable } from "../hooks/useResizable";
import type { Message } from "../types";
import { fmtTime, tokenFmt, isDiffResult } from "./sessions/helpers";

import { SessionSidebar } from "./sessions/components/SessionSidebar";
import { RightPanel, type RsTab } from "./sessions/components/RightPanel";
import { BottomPanel, type BpTab } from "./sessions/components/BottomPanel";
import { ChatPage } from "./sessions/pages/ChatPage";
import { AgentsPage } from "./sessions/pages/AgentsPage";
import { UsagePage } from "./sessions/pages/UsagePage";

export function SessionsView({
  sessionId,
  page,
}: {
  sessionId: string | null;
  page: "chat" | "agents" | "usage" | null;
}) {
  const { sidebarCollapsed } = useUIStore();
  const [rsCollapsed, setRsCollapsed] = useState(false);
  const [bpCollapsed, setBpCollapsed] = useState(false);
  const [rsTab, setRsTab] = useState<RsTab>("overview");
  const [bpTab, setBpTab] = useState<BpTab>("terminal");

  // Bug 17: resizable panels — left sidebar, right sidebar, bottom panel
  const sidebar = useResizable({
    axis: "x", min: 160, max: 400, initial: 220,
    direction: "right", persistKey: "olympus-sidebar-w",
  });
  const rightPanel = useResizable({
    axis: "x", min: 200, max: 450, initial: 279,
    direction: "left", persistKey: "olympus-rsidebar-w",
  });
  const bottomPanel = useResizable({
    axis: "y", min: 80, max: 400, initial: 152,
    direction: "down", persistKey: "olympus-bpanel-h",
  });

  return (
    <>
      {/* ── View-owned left sidebar ─────────────────────────────── */}
      {!sidebarCollapsed && (
        <SessionSidebar
          width={sidebar.size}
          activeSessionId={sessionId}
          onResizeStart={sidebar.onResizeStart}
        />
      )}

      {/* ── Viewport layout ─────────────────────────────────────── */}
      <div className="viewport">
        {page === "agents" ? (
          <div className="view on" data-view="sessions" style={{ flexDirection: "column" }}>
            <AgentsPage />
          </div>
        ) : page === "usage" ? (
          <div className="view on" data-view="sessions" style={{ flexDirection: "column" }}>
            <UsagePage />
          </div>
        ) : sessionId ? (
          <SessionChatLayout
            sessionId={sessionId}
            rsCollapsed={rsCollapsed}
            bpCollapsed={bpCollapsed}
            rsTab={rsTab}
            bpTab={bpTab}
            rsWidth={rightPanel.size}
            bpHeight={bottomPanel.size}
            onRsResizeStart={rightPanel.onResizeStart}
            onBpResizeStart={bottomPanel.onResizeStart}
            onToggleRs={() => setRsCollapsed((v) => !v)}
            onToggleBp={() => setBpCollapsed((v) => !v)}
            onRsTabChange={setRsTab}
            onBpTabChange={setBpTab}
            onCloseBp={() => setBpCollapsed(true)}
          />
        ) : (
          <SessionEmptyPane />
        )}
      </div>
    </>
  );
}

/**
 * The chat viewport layout: vp-head + vp-body (chatcol + right sidebar)
 * + bottom panel. The chatcol content (transcript + composer) is Page-owned
 * (ChatPage); the surrounding layout and right/bottom panels are View-owned.
 */
function SessionChatLayout({
  sessionId,
  rsCollapsed,
  bpCollapsed,
  rsTab,
  bpTab,
  rsWidth,
  bpHeight,
  onRsResizeStart,
  onBpResizeStart,
  onToggleRs,
  onToggleBp,
  onRsTabChange,
  onBpTabChange,
  onCloseBp,
}: {
  sessionId: string;
  rsCollapsed: boolean;
  bpCollapsed: boolean;
  rsTab: RsTab;
  bpTab: BpTab;
  rsWidth: number;
  bpHeight: number;
  onRsResizeStart: (e: React.MouseEvent) => void;
  onBpResizeStart: (e: React.MouseEvent) => void;
  onToggleRs: () => void;
  onToggleBp: () => void;
  onRsTabChange: (t: RsTab) => void;
  onBpTabChange: (t: BpTab) => void;
  onCloseBp: () => void;
}) {
  const { data: session } = useSession(sessionId);
  const { data: msgData } = useMessages(sessionId);
  const { data: agentsData } = useAgents();
  const messages = msgData?.messages ?? [];
  const navigate = useNavigate();

  // Provider for the session's bound agent → logo glyph
  const sessionAgentInfo = agentsData?.agents.find(
    (a) => a.id === session?.agent,
  );
  const agentLogo = agentBrand(sessionAgentInfo?.kind, sessionAgentInfo?.provider);

  // Derived artifact list from messages
  const artifacts = React.useMemo(() => {
    const seen = new Map<string, "new" | "modified">();
    for (const m of messages) {
      if (!m.toolCalls) continue;
      for (const tc of m.toolCalls) {
        if (tc.name === "patch" || tc.name === "write_file" || tc.name === "edit_file") {
          const args = tc.args as Record<string, unknown> | null;
          const path =
            typeof args === "object" && args && typeof args.path === "string"
              ? args.path
              : null;
          if (!path) continue;
          const isNew = tc.name === "write_file" && !tc.result?.includes("@@");
          seen.set(path, isNew ? "new" : "modified");
        }
      }
    }
    return Array.from(seen.entries()).map(([path, status]) => ({ path, status }));
  }, [messages]);

  return (
    <div
      className="view on chat-view"
      data-view="sessions"
      data-session-id={sessionId}
      style={{ flexDirection: "column" }}
    >
      {/* ── vp-head ─────────────────────────────────────────────── */}
      <div className="vp-head">
        <div className="vp-left">
          <button
            type="button"
            className="icobtn"
            style={{ padding: 0 }}
            onClick={() => void navigate({ to: "/sessions" })}
            title="Back"
          >
            <Icon name="chevron-left" />
          </button>
          <span className="vp-title chat-title">{session?.title ?? "Untitled"}</span>
          {session?.agent && (
            <span className="proj-badge">
              <BrandIcon name={agentLogo} size={11} />
              {session.agent.toUpperCase()}
            </span>
          )}
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
          <button
            type="button"
            className="icobtn"
            title="Toggle bottom panel"
            onClick={onToggleBp}
          >
            <Icon name="panel-bottom" size={14} />
          </button>
          <button
            type="button"
            className="icobtn"
            title="Toggle right panel"
            onClick={onToggleRs}
          >
            <Icon name="panel-right" size={14} />
          </button>
        </div>
      </div>

      {/* ── vp-body ─────────────────────────────────────────────── */}
      <div className="vp-body">
        {/* chatcol — Page content (ChatPage) + View-owned bottom panel */}
        <div className="chatcol" style={{ display: "flex", flexDirection: "column" }}>
          {/* ChatPage owns the transcript + composer */}
          <ChatPage sessionId={sessionId} />

          {/* View-owned bottom panel */}
          {!bpCollapsed && (
            <>
              <div className="rz-y" onMouseDown={onBpResizeStart} />
              <BottomPanel
                sessionId={sessionId}
                height={bpHeight}
                tab={bpTab}
                onTabChange={onBpTabChange}
                onClose={onCloseBp}
              />
            </>
          )}
        </div>

        {/* View-owned right sidebar */}
        {!rsCollapsed && (
          <>
            <div className="rz-x" onMouseDown={onRsResizeStart} />
            <RightPanel
              width={rsWidth}
              tab={rsTab}
              onTabChange={onRsTabChange}
              session={session}
              artifacts={artifacts}
              messages={messages}
            />
          </>
        )}
      </div>
    </div>
  );
}

/** Empty pane when no session is selected. */
export function SessionEmptyPane() {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Sessions</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name="message-square" size={32} />
          </div>
          <div className="empty-state-title">Select a session</div>
          <div className="empty-state-msg">
            Choose a session from the sidebar or create a new one to start
            chatting.
          </div>
        </div>
      </div>
    </>
  );
}
