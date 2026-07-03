// Fleet-internal helpers — shared by GridPage and NodeDetailPage.

import type { NodeStatus } from "../../types";
import { relativeTime } from "../../lib/format";

export function statusDotClass(status: NodeStatus): string {
  if (status === "online") return "ol-dot ol-dot-live";
  if (status === "draining") return "ol-dot ol-dot-warn";
  return "ol-dot ol-dot-err";
}

export function statusBadgeClass(status: NodeStatus): string {
  if (status === "online") return "ol-badge ol-badge-ok";
  if (status === "draining") return "ol-badge ol-badge-warn";
  return "ol-badge ol-badge-err";
}

export function slotBarClass(pct: number): string {
  if (pct >= 90) return "ol-bar-fill err";
  if (pct >= 70) return "ol-bar-fill warn";
  return "ol-bar-fill";
}

export function slotPct(used: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

export function heartbeatLabel(epochSecAgo: number): string {
  if (epochSecAgo < 60) return `${epochSecAgo}s ago`;
  return relativeTime(Math.floor(Date.now() / 1000) - epochSecAgo);
}

export function sessionAge(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
