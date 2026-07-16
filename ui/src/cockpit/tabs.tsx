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

/** A single live terminal tab: xterm.js bound to the operator WS. Mounted for
 *  the tab's whole lifetime; the socket/term persist across tab switches and
 *  cockpit hide (only the container's display toggles). */
function TerminalPane({ tab, visible }: { tab: CockpitTab; visible: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Mount xterm + socket ONCE per tab.
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

    const cols = term.cols || 80;
    const rows = term.rows || 24;
    const node = tab.target?.nodeId ?? "hall";
    const ws = new WebSocket(terminalWsUrl(tab.id, node, cols, rows));
    wsRef.current = ws;

    const enc = (bytes: string) => btoa(bytes);

    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data as string);
        if (msg.kind === "output" && msg.dataB64) {
          term.write(b64ToUint8(msg.dataB64));
        } else if (msg.kind === "exited") {
          const code = msg.exitCode ?? msg.error ?? "";
          term.write(`\r\n\x1b[90m[process exited ${code}]\x1b[0m\r\n`);
        }
      } catch {
        /* ignore */
      }
    };
    ws.onclose = () => {
      term.write("\r\n\x1b[90m[disconnected]\x1b[0m\r\n");
    };

    // Keystrokes → server (base64).
    const dataSub = term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ kind: "input", dataB64: enc(data) }));
      }
    });
    // Resize → server.
    const resizeSub = term.onResize(({ cols, rows }) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ kind: "resize", cols, rows }));
      }
    });

    return () => {
      dataSub.dispose();
      resizeSub.dispose();
      ws.close();
      term.dispose();
      termRef.current = null;
      wsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.id]);

  // Refit when this tab becomes visible or the window resizes.
  useEffect(() => {
    if (!visible) return;
    const refit = () => {
      try {
        fitRef.current?.fit();
        termRef.current?.focus();
      } catch {
        /* ignore */
      }
    };
    refit();
    window.addEventListener("resize", refit);
    return () => window.removeEventListener("resize", refit);
  }, [visible]);

  return <div className="cockpit-term" ref={containerRef} />;
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
