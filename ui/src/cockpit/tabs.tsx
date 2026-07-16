// Cockpit tab-kind registry (ADR 0021) — kind → renderer + metadata.
//
// Built-in kinds: terminal (live PTY), browser (iframe), editor (placeholder
// until the node file API lands). Plugins extend the cockpit by calling
// `registerCockpitTabKind` at module load — the new-tab menu and the pane
// switch both read from this registry, so no core edits are needed.
//
// Renderers stay MOUNTED for the tab's whole lifetime (Cockpit only toggles
// display) — that is what keeps a live shell/browser alive across tab
// switches and cockpit hide. Renderers must therefore be cheap when hidden.

import React, { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Icon, type IconName } from "../components/Icon";
import { terminalWsUrl } from "../api";
import { useCockpit, type CockpitTab } from "./store";

export interface CockpitTabKind {
  kind: string;
  /** Menu label ("Terminal", "Browser", …). */
  label: string;
  icon: IconName;
  /** Whether opening this kind requires picking a node (terminal: yes). */
  needsNode: boolean;
  /** Pane renderer. Mounted once per tab; `visible` toggles with the active tab. */
  render: (props: { tab: CockpitTab; visible: boolean }) => React.ReactElement;
}

const registry = new Map<string, CockpitTabKind>();

export function registerCockpitTabKind(def: CockpitTabKind): void {
  registry.set(def.kind, def);
}

export function getCockpitTabKind(kind: string): CockpitTabKind | undefined {
  return registry.get(kind);
}

export function listCockpitTabKinds(): CockpitTabKind[] {
  return [...registry.values()];
}

// ── Terminal ──────────────────────────────────────────────────────────

/** xterm theme derived from the live token layer so the terminal follows the
 *  active theme instead of hardcoding a palette. Read at mount. */
function xtermThemeFromTokens(): { background: string; foreground: string } {
  const cs = getComputedStyle(document.documentElement);
  const bg = cs.getPropertyValue("--bg").trim() || "#0A0A0B";
  const fg = cs.getPropertyValue("--text").trim() || "#E6E6E6";
  return { background: bg, foreground: fg };
}

/** Custom WS close code: the client is closing the tab permanently — kill
 *  the tmux session. Normal close (1000) just detaches. */
const CLOSE_CODE_EXPLICIT = 4000;

/** A stable terminalId stored in tab.state.terminalId. Survives page reload
 *  so a persisted tab reattaches to the same tmux session. */
function stableTerminalId(tab: CockpitTab): string {
  const existing = tab.state?.terminalId as string | undefined;
  if (existing) return existing;
  // Fallback: the tab id itself is stable across reloads (persisted).
  return tab.id;
}

type ConnState = "connecting" | "connected" | "reconnecting" | "disconnected";

/** A single live terminal tab: xterm.js bound to the operator WS. Mounted for
 *  the tab's whole lifetime; the socket/term persist across tab switches and
 *  cockpit hide (only the container's display toggles).
 *
 *  Auto-reconnects with exponential backoff + jitter on socket close unless
 *  the user explicitly closed the tab (code 4000). */
function TerminalPane({ tab, visible }: { tab: CockpitTab; visible: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const closedRef = useRef(false); // explicit close → stop reconnecting
  const backoffRef = useRef(0);
  const reconnectTimerRef = useRef<number | null>(null);

  const [connState, setConnState] = useState<ConnState>("connecting");
  const [persistent, setPersistent] = useState(true);
  const updateTab = useCockpit((s) => s.updateTab);

  const terminalId = stableTerminalId(tab);
  const node = tab.target?.nodeId ?? "hall";

  // Connect / reconnect to the WS. Called on mount and on socket close.
  const connect = useCallback(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term) return;

    const cols = term.cols || 80;
    const rows = term.rows || 24;
    const ws = new WebSocket(terminalWsUrl(terminalId, node, cols, rows));
    wsRef.current = ws;

    if (closedRef.current) return; // tab closed, don't reconnect
    setConnState(backoffRef.current > 0 ? "reconnecting" : "connecting");

    ws.onopen = () => {
      backoffRef.current = 0;
      setConnState("connected");
    };

    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data as string);
        if (msg.kind === "output" && msg.dataB64) {
          term.write(b64ToUint8(msg.dataB64));
        } else if (msg.kind === "attached" && typeof msg.persistent === "boolean") {
          setPersistent(msg.persistent);
        } else if (msg.kind === "exited") {
          const code = msg.exitCode ?? msg.error ?? "";
          term.write(`\r\n\x1b[90m[process exited ${code}]\x1b[0m\r\n`);
          closedRef.current = true; // shell is gone — don't reconnect
          setConnState("disconnected");
        }
      } catch {
        /* ignore */
      }
    };

    ws.onclose = (e: CloseEvent) => {
      if (closedRef.current) {
        setConnState("disconnected");
        return;
      }
      if (e.code === CLOSE_CODE_EXPLICIT) {
        // Server killed it at our request — don't reconnect.
        closedRef.current = true;
        setConnState("disconnected");
        return;
      }
      // Schedule reconnect with exponential backoff + jitter.
      const attempt = backoffRef.current++;
      const base = Math.min(1000 * 2 ** attempt, 10_000);
      const jitter = Math.random() * 500;
      const delay = base + jitter;
      setConnState("reconnecting");
      reconnectTimerRef.current = window.setTimeout(() => {
        connect();
      }, delay);
    };
  }, [terminalId, node]);

  // Mount xterm ONCE per tab.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "var(--font-mono), ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      theme: xtermThemeFromTokens(),
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    try {
      fit.fit();
    } catch {
      /* container not laid out yet */
    }
    termRef.current = term;
    fitRef.current = fit;

    // Persist the terminalId so a page reload reattaches to the same session.
    if (!tab.state?.terminalId) {
      updateTab(tab.id, { state: { ...tab.state, terminalId } });
    }

    connect();

    // Keystrokes → server (base64).
    const enc = (bytes: string) => btoa(bytes);
    const dataSub = term.onData((data) => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ kind: "input", dataB64: enc(data) }));
      }
    });
    // Resize → server.
    const resizeSub = term.onResize(({ cols, rows }) => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ kind: "resize", cols, rows }));
      }
    });

    return () => {
      dataSub.dispose();
      resizeSub.dispose();
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      // Send explicit close so the server kills the session.
      closedRef.current = true;
      if (wsRef.current) {
        wsRef.current.close(CLOSE_CODE_EXPLICIT, "tab-closed");
        wsRef.current = null;
      }
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.id]);

  // Refit when this tab becomes visible, and on ResizeObserver of the container.
  useEffect(() => {
    if (!visible) return;
    const container = containerRef.current;
    if (!container) return;

    const refit = () => {
      try {
        fitRef.current?.fit();
        termRef.current?.focus();
      } catch {
        /* ignore */
      }
    };
    refit();

    // Window resize (fallback for when RO isn't supported).
    window.addEventListener("resize", refit);

    // ResizeObserver: catches cockpit drag-resize that window resize misses.
    let ro: ResizeObserver | null = null;
    if ("ResizeObserver" in window) {
      ro = new ResizeObserver(() => {
        refit();
      });
      ro.observe(container);
    }

    return () => {
      window.removeEventListener("resize", refit);
      ro?.disconnect();
    };
  }, [visible]);

  return (
    <div className="cockpit-term-wrap" style={{ position: "relative", height: "100%" }}>
      <div className="cockpit-term" ref={containerRef} />
      {connState === "reconnecting" && (
        <div className="cockpit-term-status reconnecting">Reconnecting…</div>
      )}
      {connState === "connecting" && (
        <div className="cockpit-term-status">Connecting…</div>
      )}
      {!persistent && connState !== "disconnected" && (
        <div className="cockpit-term-badge-nonpersistent" title="tmux not installed — shell won't survive disconnect">
          non-persistent
        </div>
      )}
    </div>
  );
}

/** Decode base64 → Uint8Array for xterm.write (bytes, not UTF-8 string). */
function b64ToUint8(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ── Browser ───────────────────────────────────────────────────────────

function normalizeUrl(raw: string): string {
  const t = raw.trim();
  if (!t) return "";
  if (/^https?:\/\//i.test(t)) return t;
  return `https://${t}`;
}

/** Simple embedded browser: URL bar + iframe. Sites that send
 *  X-Frame-Options/CSP frame-ancestors will refuse to embed — that is a
 *  browser platform limit, surfaced as a hint instead of a blank mystery. */
function BrowserPane({ tab }: { tab: CockpitTab; visible: boolean }) {
  const updateTab = useCockpit((s) => s.updateTab);
  const url = (tab.state?.url as string) ?? "";
  const [input, setInput] = useState(url);

  const go = useCallback(() => {
    const next = normalizeUrl(input);
    if (!next) return;
    setInput(next);
    updateTab(tab.id, {
      state: { ...tab.state, url: next },
      title: next.replace(/^https?:\/\//, "").slice(0, 40) || "Browser",
    });
  }, [input, tab.id, tab.state, updateTab]);

  return (
    <div className="cockpit-browser">
      <div className="cockpit-browser-bar">
        <Icon name="globe" size={12} />
        <input
          className="cockpit-browser-url"
          placeholder="https://…"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") go();
          }}
        />
        <button type="button" className="ol-btn ol-btn-sm" onClick={go}>
          Go
        </button>
      </div>
      {url ? (
        <iframe className="cockpit-browser-frame" src={url} title={tab.title} />
      ) : (
        <div className="cockpit-pane-empty">
          <Icon name="globe" size={24} />
          <span>Enter a URL to browse. Sites that forbid embedding won't render.</span>
        </div>
      )}
    </div>
  );
}

// ── Code editor (placeholder until the node file API lands) ────────────

function EditorPane({ tab }: { tab: CockpitTab; visible: boolean }) {
  return (
    <div className="cockpit-pane-empty">
      <Icon name="file" size={24} />
      <span>
        Code editor — needs the node file API (list/read/write over the envoy
        channel) before it can browse {tab.target?.nodeId ?? "a node"}'s files.
      </span>
      <span className="gtag">COMING · CARD CP-EDITOR</span>
    </div>
  );
}

// ── Fallback for unknown kinds (plugin not loaded / stale persistence) ──

export function UnknownKindPane({ tab }: { tab: CockpitTab; visible: boolean }) {
  return (
    <div className="cockpit-pane-empty">
      <Icon name="puzzle" size={24} />
      <span>
        No renderer for tab kind "{tab.kind}" — the plugin that provides it
        isn't loaded.
      </span>
    </div>
  );
}

// ── Built-in registrations ─────────────────────────────────────────────

registerCockpitTabKind({
  kind: "terminal",
  label: "Terminal",
  icon: "terminal",
  needsNode: true,
  render: (p) => <TerminalPane {...p} />,
});

registerCockpitTabKind({
  kind: "browser",
  label: "Browser",
  icon: "globe",
  needsNode: false,
  render: (p) => <BrowserPane {...p} />,
});

registerCockpitTabKind({
  kind: "editor",
  label: "Code editor",
  icon: "file",
  needsNode: true,
  render: (p) => <EditorPane {...p} />,
});
