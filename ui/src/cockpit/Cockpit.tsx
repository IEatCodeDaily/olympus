// Operator Cockpit (ADR 0021) — a single floating, tabbed, operator-only
// workspace mounted ONCE at the AppShell root (outside the surface switch), so
// it persists across every view. Toggle from the top-right button; hiding never
// disposes tabs (their xterm instances + sockets stay alive).
//
// Tabs are kind-polymorphic (terminal / browser / editor, plugin-extensible)
// via the registry in cockpit/tabs.tsx. Terminals are REAL shells: xterm.js
// bound to the dedicated operator WebSocket (/ws/operator/terminals/:id),
// relayed to an Envoy-owned PTY (or the Hall-local PTY for "hall").
//
// Operator-only. No agent ever drives this.

import React, { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { fetchTerminalTargets, type TerminalTarget } from "../api";
import { useCockpit, type CockpitTab } from "./store";
import { getCockpitTabKind, listCockpitTabKinds, UnknownKindPane } from "./tabs";

const MIN_W = 480;
const MIN_H = 280;

function isInteractiveTitlebarTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest("button,a,input,select,textarea,[role='button'],[role='menuitem'],[data-cockpit-interactive]"));
}

export function Cockpit() {
  const { open, geometry, tabs, activeTabId, setGeometry, closeTab, setActiveTab, toggle } =
    useCockpit();

  const dragRef = useRef<{ mode: "move" | "resize"; sx: number; sy: number; g: typeof geometry } | null>(
    null,
  );

  const onMove = useCallback(
    (e: PointerEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const dx = e.clientX - d.sx;
      const dy = e.clientY - d.sy;
      if (d.mode === "move") {
        setGeometry({ x: Math.max(0, d.g.x + dx), y: Math.max(0, d.g.y + dy) });
      } else {
        setGeometry({ w: Math.max(MIN_W, d.g.w + dx), h: Math.max(MIN_H, d.g.h + dy) });
      }
    },
    [setGeometry],
  );

  const startDrag = (mode: "move" | "resize") => (e: React.PointerEvent) => {
    if (mode === "move" && isInteractiveTitlebarTarget(e.target)) return;
    e.preventDefault();
    dragRef.current = { mode, sx: e.clientX, sy: e.clientY, g: geometry };
    const up = () => {
      dragRef.current = null;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", up);
  };

  const active = tabs.find((t) => t.id === activeTabId) ?? tabs[0] ?? null;

  return (
    <div
      className={`cockpit${open ? "" : " is-hidden"}`}
      style={{ left: geometry.x, top: geometry.y, width: geometry.w, height: geometry.h }}
      role="dialog"
      aria-label="Operator cockpit"
      aria-hidden={!open}
    >
      <div className="cockpit-titlebar" onPointerDown={startDrag("move")}>
        <span className="cockpit-title">
          <Icon name="terminal" size={13} /> Cockpit
        </span>
        <div className="cockpit-tabs">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`cockpit-tab ${t.id === active?.id ? "on" : ""}`}
              onClick={() => setActiveTab(t.id)}
              title={t.title}
            >
              <Icon name={getCockpitTabKind(t.kind)?.icon ?? "puzzle"} size={11} />
              <span className="cockpit-tab-label">{t.title}</span>
              <span
                className="cockpit-tab-close"
                role="button"
                aria-label={`Close ${t.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.id);
                }}
              >
                <Icon name="x" size={9} />
              </span>
            </button>
          ))}
          <NewTabButton />
        </div>
        <button
          type="button"
          className="cockpit-icobtn cockpit-hide"
          title="Hide cockpit (tabs stay open)"
          aria-label="Hide cockpit"
          onClick={toggle}
        >
          <Icon name="x" size={13} />
        </button>
      </div>

      <div className="cockpit-body">
        {tabs.length === 0 ? (
          <div className="cockpit-empty">
            <p>No tabs open.</p>
            <NewTabButton inline />
          </div>
        ) : (
          // Every tab stays mounted; only the active one is visible — this is
          // what keeps a live shell alive when you switch tabs.
          tabs.map((t) => {
            const def = getCockpitTabKind(t.kind);
            const visible = t.id === active?.id;
            return (
              <div key={t.id} className="cockpit-pane" style={{ display: visible ? "block" : "none" }}>
                {def ? def.render({ tab: t, visible }) : <UnknownKindPane tab={t} visible={visible} />}
              </div>
            );
          })
        )}
      </div>

      <div className="cockpit-resize" onPointerDown={startDrag("resize")} aria-hidden />
    </div>
  );
}

function NewTabButton({ inline }: { inline?: boolean }) {
  const addTab = useCockpit((s) => s.addTab);
  const [open, setOpen] = useState(false);
  const [nodeKind, setNodeKind] = useState<string | null>(null);
  const [targets, setTargets] = useState<TerminalTarget[] | null>(null);
  const rootRef = useRef<HTMLSpanElement>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearCloseTimer = () => {
    if (closeTimer.current) {
      clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };
  const closeMenu = () => {
    clearCloseTimer();
    setOpen(false);
    setNodeKind(null);
  };
  const scheduleCloseSubmenu = () => {
    clearCloseTimer();
    closeTimer.current = setTimeout(() => setNodeKind(null), 180);
  };

  useEffect(() => () => clearCloseTimer(), []);

  // Load node targets when a node submenu opens.
  useEffect(() => {
    if (!nodeKind) return;
    let alive = true;
    setTargets(null);
    fetchTerminalTargets()
      .then((t) => {
        if (alive) setTargets(t);
      })
      .catch(() => {
        if (alive) setTargets([{ id: "hall", label: "Hall", kind: "hall", default: true }]);
      });
    return () => {
      alive = false;
    };
  }, [nodeKind]);

  // Close on click-outside / Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) closeMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const openKind = (kind: string) => {
    const def = getCockpitTabKind(kind);
    if (!def) return;
    if (def.needsNode) {
      clearCloseTimer();
      setNodeKind(kind);
    } else {
      addTab({ kind, label: def.label });
      closeMenu();
    }
  };

  const spawnOnNode = (kind: string, target: TerminalTarget) => {
    addTab({ kind, node: target.id, label: target.label });
    closeMenu();
  };

  return (
    <span className="cockpit-newtab" ref={rootRef} data-cockpit-interactive onMouseEnter={clearCloseTimer} onMouseLeave={scheduleCloseSubmenu}>
      <button
        type="button"
        className={inline ? "cockpit-newbtn-inline" : "cockpit-icobtn"}
        title="New tab"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          clearCloseTimer();
          setOpen((value) => !value);
        }}
      >
        <Icon name="plus" size={inline ? 13 : 12} /> {inline ? "New tab" : null}
      </button>
      {open && (
        <div className="cockpit-menu menu on" role="menu">
          <div className="cockpit-menu-head">New tab</div>
          {listCockpitTabKinds().map((k) => (
            <button
              key={k.kind}
              type="button"
              role="menuitem"
              aria-haspopup={k.needsNode ? "menu" : undefined}
              aria-expanded={k.needsNode ? nodeKind === k.kind : undefined}
              className="cockpit-menu-item"
              onMouseEnter={() => (k.needsNode ? openKind(k.kind) : setNodeKind(null))}
              onFocus={() => (k.needsNode ? openKind(k.kind) : setNodeKind(null))}
              onClick={() => openKind(k.kind)}
              onKeyDown={(e) => {
                if (k.needsNode && (e.key === "Enter" || e.key === "ArrowRight")) {
                  e.preventDefault();
                  openKind(k.kind);
                }
              }}
            >
              <Icon name={k.icon} size={12} />
              <span>{k.label}</span>
              {k.needsNode && <Icon name="chevron-right" size={11} />}
            </button>
          ))}
          {nodeKind && (
            <div className="cockpit-submenu menu on" role="menu" onMouseEnter={clearCloseTimer}>
              <div className="cockpit-menu-head">{getCockpitTabKind(nodeKind)?.label} on…</div>
              {targets === null && <div className="cockpit-menu-item is-loading">loading…</div>}
              {targets?.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="menuitem"
                  className="cockpit-menu-item"
                  onClick={() => spawnOnNode(nodeKind, t)}
                >
                  <Icon name="server" size={12} />
                  <span>{t.label}</span>
                  {t.default && <span className="cockpit-menu-default">default</span>}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </span>
  );
}
