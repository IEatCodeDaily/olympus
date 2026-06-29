// Formatting helpers — relative time, token counts, source colors.

import type { SessionSource } from "../types";

export function relativeTime(epochSec: number): string {
  const now = Date.now() / 1000;
  const diff = now - epochSec;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 86400 * 2) return "yesterday";
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}d ago`;
  const d = new Date(epochSec * 1000);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

export function formatTime(epochSec: number): string {
  return new Date(epochSec * 1000).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export const SOURCE_META: Record<
  SessionSource,
  { label: string; color: string; glow: string }
> = {
  cli: { label: "CLI", color: "#7dd3fc", glow: "rgba(125,211,252,0.15)" },
  telegram: { label: "Telegram", color: "#5ec8f2", glow: "rgba(94,200,242,0.15)" },
  discord: { label: "Discord", color: "#9b8cf2", glow: "rgba(155,140,242,0.15)" },
  webui: { label: "WebUI", color: "#5eead4", glow: "rgba(94,234,212,0.15)" },
  cron: { label: "Cron", color: "#fcd34d", glow: "rgba(252,211,77,0.15)" },
  subagent: { label: "Subagent", color: "#f0abfc", glow: "rgba(240,171,252,0.15)" },
  api_server: { label: "API", color: "#a3a3a3", glow: "rgba(163,163,163,0.15)" },
  acp: { label: "ACP", color: "#86efac", glow: "rgba(134,239,172,0.15)" },
  olympus: { label: "Olympus", color: "#22d3ee", glow: "rgba(34,211,238,0.18)" },
};

export const ALL_SOURCES: SessionSource[] = [
  "olympus",
  "cli",
  "telegram",
  "discord",
  "webui",
  "cron",
  "subagent",
  "api_server",
  "acp",
];
