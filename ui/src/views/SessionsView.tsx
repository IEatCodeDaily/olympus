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

import React, { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import {
  DockviewReact,
  type DockviewApi,
  type DockviewDidDropEvent,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from "dockview-react";
import "dockview-react/dist/styles/dockview.css";
import { Icon } from "../components/Icon";
import { BrandIcon, agentBrand } from "../components/BrandIcons";
import { useUIStore } from "../store";
import { useSession, useMessages, useAgents } from "../hooks/queries";
import { useResizable } from "../hooks/useResizable";
import { getLocalUiState, loadWorkspaceState, saveWorkspaceState } from "../lib/uiState";
import { readSessionPanelState, writeSessionPanelState } from "../workbench/sessionPanelState";

import { SessionSidebar } from "./sessions/components/SessionSidebar";
import { RightPanel, type RsTab } from "./sessions/components/RightPanel";
import { BottomPanel, type BpTab } from "./sessions/components/BottomPanel";
import { ChatPage } from "./sessions/pages/ChatPage";
import { AgentsPage } from "./sessions/pages/AgentsPage";
import { UsagePage } from "./sessions/pages/UsagePage";
import { HistoryPage } from "./sessions/pages/HistoryPage";

type DockLayout = ReturnType<DockviewApi["toJSON"]>;

interface SessionPanelParams {
  sessionId: string;
}

let savedSessionsLayout: DockLayout | null = null;

export function SessionsView({
  sessionId,
  page,
}: {
  sessionId: string | null;
  page: "chat" | "agents" | "usage" | "history" | null;
}) {
  const { sidebarCollapsed } = useUIStore();
  const navigate = useNavigate();
  const apiRef = useRef<DockviewApi | null>(null);
  const [openSessionIds, setOpenSessionIds] = useState<Set<string>>(() => new Set());

  const syncOpenSessions = useCallback((api: DockviewApi) => {
    setOpenSessionIds(new Set(api.panels.flatMap((panel) => {
      const id = (panel.params as SessionPanelParams | undefined)?.sessionId;
      return id ? [id] : [];
    })));
  }, []);

  // Bug 17: resizable panels — left sidebar, right sidebar, bottom panel
  const sidebar = useResizable({
    axis: "x", min: 160, max: 400, initial: 220,
    direction: "right", persistKey: "olympus-sidebar-w",
  });
  const persist = useCallback(() => {
    const api = apiRef.current;
    if (!api) return;
    savedSessionsLayout = api.toJSON();
    saveWorkspaceState("sessions", savedSessionsLayout);
  }, []);

  const openSessionPanel = useCallback((id: string, drop?: DockviewDidDropEvent) => {
    const api = apiRef.current;
    if (!api) return;
    const panelId = `session:${id}`;
    const existing = api.getPanel(panelId);
    if (existing) {
      existing.api.setActive();
      syncOpenSessions(api);
      return;
    }
    const panel = api.addPanel({
      id: panelId,
      title: id,
      component: "session-panel",
      params: { sessionId: id } satisfies SessionPanelParams,
      ...(drop?.group ? {
        position: { referenceGroup: drop.group, direction: dropDirection(drop.position) },
      } : {}),
    });
    panel.api.setActive();
    syncOpenSessions(api);
    persist();
  }, [persist, syncOpenSessions]);

  useEffect(() => {
    if (sessionId) openSessionPanel(sessionId);
  }, [openSessionPanel, sessionId]);

  useEffect(() => () => persist(), [persist]);

  const handleReady = useCallback((event: DockviewReadyEvent) => {
    apiRef.current = event.api;
    const local = savedSessionsLayout ?? getLocalUiState<DockLayout>("sessions");
    if (local) {
      try {
        event.api.fromJSON(local);
        syncOpenSessions(event.api);
      } catch {
        savedSessionsLayout = null;
      }
    }
    void loadWorkspaceState<DockLayout>("sessions").then((remote) => {
      if (!remote || apiRef.current !== event.api) return;
      try {
        event.api.fromJSON(remote);
        syncOpenSessions(event.api);
        savedSessionsLayout = remote;
        if (sessionId) openSessionPanel(sessionId);
      } catch {
        // Ignore incompatible remote layouts.
      }
    });
    if (sessionId) openSessionPanel(sessionId);
    const layoutDisposable = event.api.onDidLayoutChange(() => persist());
    const activeDisposable = event.api.onDidActivePanelChange(({ panel }) => {
      const params = panel?.params as SessionPanelParams | undefined;
      if (params?.sessionId) {
        void navigate({ to: "/sessions/$sessionId", params: { sessionId: params.sessionId } });
      }
    });
    const removeDisposable = event.api.onDidRemovePanel(() => {
      syncOpenSessions(event.api);
    });
    const dragOverDisposable = event.api.onUnhandledDragOver((dragEvent) => {
      if (hasDragType(dragEvent.nativeEvent, "application/x-olympus-session")) dragEvent.accept();
    });
    const dropDisposable = event.api.onDidDrop((dropEvent) => {
      const payload = dragPayload(dropEvent.nativeEvent, "application/x-olympus-session") as { sessionId?: string } | null;
      if (!payload?.sessionId) return;
      openSessionPanel(payload.sessionId, dropEvent);
      void navigate({ to: "/sessions/$sessionId", params: { sessionId: payload.sessionId } });
    });
    return () => {
      layoutDisposable.dispose();
      activeDisposable.dispose();
      removeDisposable.dispose();
      dragOverDisposable.dispose();
      dropDisposable.dispose();
    };
  }, [navigate, openSessionPanel, persist, sessionId, syncOpenSessions]);

  return (
    <>
      {/* ── View-owned left sidebar ─────────────────────────────── */}
      {!sidebarCollapsed && (
        <SessionSidebar
          width={sidebar.size}
          activeSessionId={sessionId}
          openSessionIds={openSessionIds}
          onResizeStart={sidebar.onResizeStart}
          onResizeKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            sidebar.setSize(Math.max(160, Math.min(400, sidebar.size + (event.key === "ArrowRight" ? 10 : -10))));
          }}
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
        ) : page === "history" ? (
          <div className="view on" data-view="sessions" style={{ flexDirection: "column" }}>
            <HistoryPage />
          </div>
        ) : (
          <div className="sessions-dock-shell">
            <DockviewReact
              className="dockview-theme-abyss olympus-dockview sessions-dockview"
              components={{ "session-panel": SessionDockPanel }}
              onReady={handleReady}
            />
            {!sessionId && openSessionIds.size === 0 && <div className="sessions-dock-empty"><SessionEmptyPane /></div>}
          </div>
        )}
      </div>
    </>
  );
}

function dropDirection(position: "top" | "bottom" | "left" | "right" | "center"): "left" | "right" | "above" | "below" | "within" {
  return position === "top" ? "above" : position === "bottom" ? "below" : position === "center" ? "within" : position;
}

function hasDragType(event: globalThis.DragEvent | PointerEvent, type: string): boolean {
  return event instanceof globalThis.DragEvent && event.dataTransfer?.types.includes(type) === true;
}

function dragPayload(event: globalThis.DragEvent | PointerEvent, type: string): unknown | null {
  if (!(event instanceof globalThis.DragEvent)) return null;
  try {
    const value = event.dataTransfer?.getData(type);
    return value ? JSON.parse(value) : null;
  } catch {
    return null;
  }
}

function SessionDockPanel({ params }: IDockviewPanelProps<SessionPanelParams>) {
  const [rsCollapsed, setRsCollapsed] = useSessionPanelState(params.sessionId, "rsCollapsed", false);
  const [bpCollapsed, setBpCollapsed] = useSessionPanelState(params.sessionId, "bpCollapsed", false);
  const [rsTab, setRsTab] = useSessionPanelState<RsTab>(params.sessionId, "rsTab", "overview");
  const [bpTab, setBpTab] = useSessionPanelState<BpTab>(params.sessionId, "bpTab", "terminal");
  const rightPanel = useResizable({
    axis: "x", min: 200, max: 450, initial: 279,
    direction: "left", persistKey: `olympus-session-${params.sessionId}-rsidebar-w`,
  });
  const bottomPanel = useResizable({
    axis: "y", min: 80, max: 400, initial: 152,
    direction: "down", persistKey: `olympus-session-${params.sessionId}-bpanel-h`,
  });

  return (
    <SessionChatLayout
      sessionId={params.sessionId}
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
  );
}

function useSessionPanelState<T>(sessionId: string, key: string, initial: T): [T, React.Dispatch<React.SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() => readSessionPanelState(sessionId, key, initial));
  useEffect(() => {
    writeSessionPanelState(sessionId, key, value);
  }, [key, sessionId, value]);
  return [value, setValue];
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
