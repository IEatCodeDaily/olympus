// Command palette + topbar search pill (restores the topbar search regression).
//
// The concept design (docs/design/concept/olympus-app-concept.html) puts a
// clickable search pill in the topbar center that opens a ⌘K command palette
// over sessions, vaults, and nodes. The CSS (.tb-search, .pal-*) already ships
// in index.css and the store already carries paletteOpen; this wires the React.

import React, { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "./components/Icon";
import { useUIStore } from "./store";
import { useSessions, useVaults, useNodes } from "./hooks/queries";

/** Topbar search pill — click or ⌘K to open the palette. */
export function SearchPill() {
  const setPaletteOpen = useUIStore((s) => s.setPaletteOpen);
  // Global ⌘K / Ctrl+K shortcut.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPaletteOpen]);

  return (
    <button
      type="button"
      className="tb-search"
      onClick={() => setPaletteOpen(true)}
      aria-label="Search (Cmd+K)"
      title="Search sessions, vaults, nodes (⌘K)"
    >
      <Icon name="search" size={13} />
      <span className="ph">Search…</span>
      <span className="sp" />
      <span className="kbd">⌘K</span>
    </button>
  );
}

interface PalItem {
  id: string;
  label: string;
  hint: string;
  icon: "message-square" | "book" | "server";
  go: () => void;
}

/** ⌘K command palette — searches sessions, vaults, and fleet nodes. */
export function CommandPalette() {
  const open = useUIStore((s) => s.paletteOpen);
  const setOpen = useUIStore((s) => s.setPaletteOpen);
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // Only fetch when open (cheap; these are already cached by their hooks).
  const { data: sessions } = useSessions({ limit: 50 });
  const { data: vaults } = useVaults();
  const { data: nodes } = useNodes();

  const items = useMemo<PalItem[]>(() => {
    const out: PalItem[] = [];
    for (const s of sessions?.sessions ?? []) {
      out.push({
        id: `session:${s.id}`,
        label: s.title || s.id,
        hint: "session",
        icon: "message-square",
        go: () => navigate({ to: `/sessions/${s.id}` }),
      });
    }
    for (const v of vaults?.vaults ?? []) {
      out.push({
        id: `vault:${v.id}`,
        label: v.name || v.id,
        hint: "vault",
        icon: "book",
        go: () => navigate({ to: "/vaults" }),
      });
    }
    for (const n of nodes?.nodes ?? []) {
      out.push({
        id: `node:${n.nodeId}`,
        label: n.hostname || n.nodeId,
        hint: "node",
        icon: "server",
        go: () => navigate({ to: `/fleet/${n.nodeId}` }),
      });
    }
    return out;
  }, [sessions, vaults, nodes, navigate]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items.slice(0, 40);
    return items.filter((i) => i.label.toLowerCase().includes(q) || i.hint.includes(q)).slice(0, 40);
  }, [items, query]);

  // Reset + focus on open.
  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      // focus after paint
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setActive(0);
  }, [query]);

  if (!open) return null;

  const close = () => setOpen(false);
  const choose = (i: PalItem) => {
    i.go();
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = filtered[active];
      if (item) choose(item);
    }
  };

  return (
    <div className="pal-scrim on" onClick={close}>
      <div className="pal" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Command palette">
        <div className="pal-in">
          <Icon name="search" size={14} />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Search sessions, vaults, nodes…"
            aria-label="Command palette query"
          />
          <span className="kbd">esc</span>
        </div>
        <div className="pal-list">
          {filtered.length === 0 && <div className="pal-gl">no matches</div>}
          {filtered.map((i, idx) => (
            <button
              key={i.id}
              type="button"
              className={`pal-r${idx === active ? " sel" : ""}`}
              onMouseEnter={() => setActive(idx)}
              onClick={() => choose(i)}
            >
              <Icon name={i.icon} size={13} />
              <span>{i.label}</span>
              <span className="m">{i.hint}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
