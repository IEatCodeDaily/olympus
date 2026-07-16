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
              onPointerDown={(e) => e.stopPropagation()}
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
          onPointerDown={(e) => e.stopPropagation()}
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

/** The `+` button. CLICK toggles the menu (hover was finicky: the pointer had
 *  to cross a dead gap to reach the popup and the target rows were small —
 *  see the cockpit styling review). The menu lists tab kinds; kinds that need
 *  a node (terminal, editor) expand into the node list in a second step. */
function NewTabButton({ inline }: { inline?: boolean }) {
  const addTab = useCockpit((s) => s.addTab);
  const [menu, setMenu] = useState<"closed" | "kinds" | { pickNode: string }>("closed");
  const [targets, setTargets] = useState<TerminalTarget[] | null>(null);
  const rootRef = useRef<HTMLSpanElement>(null);

  // Load node targets when a node-picking step opens.
  useEffect(() => {
    if (typeof menu !== "object") return;
    let alive = true;
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
  }, [menu]);

  // Close on click-outside / Escape.
  useEffect(() => {
    if (menu === "closed") return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setMenu("closed");
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu("closed");
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const openKind = (kind: string) => {
    const def = getCockpitTabKind(kind);
    if (!def) return;
    if (def.needsNode) {
      setTargets(null);
      setMenu({ pickNode: kind });
    } else {
      addTab({ kind, label: def.label });
      setMenu("closed");
    }
  };

  const spawnOnNode = (kind: string, target: TerminalTarget) => {
    addTab({ kind, node: target.id, label: target.label });
    setMenu("closed");
  };

  return (
    <span className="cockpit-newtab" ref={rootRef} onPointerDown={(e) => e.stopPropagation()}>
      <button
        type="button"
        className={inline ? "cockpit-newbtn-inline" : "cockpit-icobtn"}
        title="New tab"
        aria-haspopup="menu"
        aria-expanded={menu !== "closed"}
        onClick={() => setMenu(menu === "closed" ? "kinds" : "closed")}
      >
        <Icon name="plus" size={inline ? 13 : 12} /> {inline ? "New tab" : null}
      </button>
      {menu !== "closed" && (
        <div className="cockpit-menu menu on" role="menu">
          {menu === "kinds" ? (
            <>
              <div className="cockpit-menu-head">New tab</div>
              {listCockpitTabKinds().map((k) => (
                <button
                  key={k.kind}
                  type="button"
                  role="menuitem"
                  className="cockpit-menu-item"
                  onClick={() => openKind(k.kind)}
                >
                  <Icon name={k.icon} size={12} />
                  <span>{k.label}</span>
                  {k.needsNode && <Icon name="chevron-right" size={11} />}
                </button>
              ))}
            </>
          ) : (
            <>
              <button
                type="button"
                className="cockpit-menu-head cockpit-menu-back"
                onClick={() => setMenu("kinds")}
              >
                <Icon name="chevron-left" size={11} /> {getCockpitTabKind(menu.pickNode)?.label} on…
              </button>
              {targets === null && <div className="cockpit-menu-item is-loading">loading…</div>}
              {targets?.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="menuitem"
                  className="cockpit-menu-item"
                  onClick={() => spawnOnNode(menu.pickNode, t)}
                >
                  <Icon name="server" size={12} />
                  <span>{t.label}</span>
                  {t.default && <span className="cockpit-menu-default">default</span>}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </span>
  );
}
