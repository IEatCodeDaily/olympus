/* ═══════════════════════════════════════════════════════════
   Shared UI primitives for Olympus views
   ═══════════════════════════════════════════════════════════ */

import type { SessionSource } from "./types";

/* ── Source badge colors ── */
const SOURCE_COLORS: Record<SessionSource, string> = {
  cli: "var(--src-cli)",
  telegram: "var(--src-telegram)",
  discord: "var(--src-discord)",
  webui: "var(--src-webui)",
  cron: "var(--src-cron)",
  subagent: "var(--src-subagent)",
  api_server: "var(--src-api_server)",
  acp: "var(--src-acp)",
  olympus: "var(--src-olympus)",
};

const SOURCE_LABELS: Record<SessionSource, string> = {
  cli: "CLI",
  telegram: "Telegram",
  discord: "Discord",
  webui: "WebUI",
  cron: "Cron",
  subagent: "Subagent",
  api_server: "API",
  acp: "ACP",
  olympus: "Olympus",
};

export function sourceColor(s: SessionSource): string {
  return SOURCE_COLORS[s] ?? "var(--text-tertiary)";
}

export function sourceLabel(s: SessionSource): string {
  return SOURCE_LABELS[s] ?? s;
}

export const ALL_SOURCES: SessionSource[] = [
  "cli", "telegram", "discord", "webui", "cron", "subagent", "api_server", "acp",
];

/* ── Relative time formatting ── */
export function relativeTime(epochSec: number): string {
  const now = Date.now() / 1000;
  const diff = now - epochSec;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  if (diff < 604800) return `${Math.floor(diff / 86400)}d`;
  if (diff < 2592000) return `${Math.floor(diff / 604800)}w`;
  return new Date(epochSec * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

/* ── Token formatting ── */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1000000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1000000).toFixed(1)}M`;
}

/* ── Source dot (colored circle badge) ── */
export function SourceDot({ source, size = 8 }: { source: SessionSource; size?: number }) {
  return (
    <span
      style={{
        display: "inline-block",
        width: size,
        height: size,
        borderRadius: "50%",
        backgroundColor: sourceColor(source),
        flexShrink: 0,
      }}
    />
  );
}

/* ── Source badge (pill with label) ── */
export function SourceBadge({ source, active }: { source: SessionSource; active?: boolean }) {
  const color = sourceColor(source);
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "4px",
        padding: "1px 6px",
        fontSize: "10px",
        fontFamily: "var(--font-mono)",
        fontWeight: 500,
        letterSpacing: "0.02em",
        lineHeight: "16px",
        borderRadius: "var(--radius-sm)",
        border: `1px solid ${active ? color : "var(--border-subtle)"}`,
        color: active ? color : "var(--text-tertiary)",
        background: active ? "transparent" : "var(--bg-2)",
        cursor: "pointer",
        transition: `all var(--dur-fast) var(--ease-out)`,
        userSelect: "none",
      }}
    >
      {sourceLabel(source)}
    </span>
  );
}

/* ── Model pill ── */
export function ModelPill({ model }: { model: string | null }) {
  if (!model) return null;
  // Shorten common prefixes for density
  const short = model
    .replace(/^claude-/, "")
    .replace(/^gpt-/, "")
    .replace(/^gemini-/, "")
    .replace(/^deepseek-/, "")
    .replace(/^llama-/, "L")
    .replace(/^glm-/, "GLM-")
    .replace(/-\d{8}$/, ""); // strip date suffixes

  return (
    <span
      className="model-pill"
      style={{
        display: "inline-flex",
        alignItems: "center",
        padding: "0 5px",
        fontSize: "10px",
        fontFamily: "var(--font-mono)",
        fontWeight: 400,
        lineHeight: "16px",
        borderRadius: "var(--radius-sm)",
        color: "var(--text-secondary)",
        background: "var(--bg-3)",
        border: "1px solid var(--border-faint)",
      }}
    >
      {short}
    </span>
  );
}

/* ── Spinner (minimal, no generic circle) ── */
export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <span
      style={{
        display: "inline-block",
        width: size,
        height: size,
        borderRadius: "50%",
        border: "1.5px solid var(--border-default)",
        borderTopColor: "var(--accent)",
        animation: "spin 0.8s linear infinite",
      }}
    />
  );
}

/* ── Collapsible section ── */
export function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      style={{
        transform: open ? "rotate(90deg)" : "rotate(0deg)",
        transition: `transform var(--dur-fast) var(--ease-out)`,
        flexShrink: 0,
      }}
    >
      <path d="M3 1.5L7 5L3 8.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/* ── Highlighted search match ── */
export function Highlight({ text, query }: { text: string; query: string }) {
  if (!query.trim()) return <>{text}</>;
  const parts: React.ReactNode[] = [];
  const lowerQ = query.toLowerCase();
  const lowerText = text.toLowerCase();
  let lastIdx = 0;
  let idx = lowerText.indexOf(lowerQ);
  let key = 0;
  while (idx !== -1) {
    if (idx > lastIdx) parts.push(text.slice(lastIdx, idx));
    parts.push(
      <mark key={key++} style={{
        background: "var(--accent-dim)",
        color: "var(--accent-hover)",
        borderRadius: "2px",
        padding: "0 1px",
      }}>
        {text.slice(idx, idx + query.length)}
      </mark>
    );
    lastIdx = idx + query.length;
    idx = lowerText.indexOf(lowerQ, lastIdx);
  }
  if (lastIdx < text.length) parts.push(text.slice(lastIdx));
  return <>{parts}</>;
}
