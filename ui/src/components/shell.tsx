// Shared shell primitives for Olympus views. View workers build on these so
// every screen feels consistent. All styling via CSS-variable classes (themeable).
//
// IMPORTANT: these emit the LIVE class vocabulary in ui/src/index.css
// (.gv-*, .empty-state*, .stat/.v/.l, .gtag[.ok|.warn|.err]). Do not invent new
// class names here — a new visual need is a new rule in index.css first.
import type { ReactNode } from "react";

/** Page header: title + optional subtitle + right-aligned actions. */
export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="gv-head">
      <span className="gv-title">{title}</span>
      {subtitle && <span className="gv-sub">{subtitle}</span>}
      {actions && <div className="gv-actions">{actions}</div>}
    </div>
  );
}

/** Empty / placeholder state: icon + message + optional CTA. */
export function EmptyState({
  icon,
  title,
  message,
  cta,
}: {
  icon?: ReactNode;
  title: string;
  message?: string;
  cta?: ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon && <div className="empty-state-icon">{icon}</div>}
      <div className="empty-state-title">{title}</div>
      {message && <div className="empty-state-msg">{message}</div>}
      {cta && <div className="empty-state-cta">{cta}</div>}
    </div>
  );
}

/** "Coming soon" placeholder badge for views whose backend epic isn't live. */
export function PlaceholderBadge({ epic }: { epic: string }) {
  return (
    <span className="gtag warn" title={`Backend: ${epic}`}>
      placeholder · {epic}
    </span>
  );
}

/** A metric chip: big mono value stacked over an uppercase label. */
export function StatPill({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="stat">
      <span className="v">{value}</span>
      <span className="l">{label}</span>
    </div>
  );
}

/**
 * Status badge. `kind` maps to the semantic .gtag color variants (ok/warn/err).
 *
 * The lookup is CASE-INSENSITIVE and covers the full fleet/session status
 * vocabulary, so a status arriving capitalized ("Running") or as a common
 * synonym ("queued", "stopped") never silently degrades to a NEUTRAL badge and
 * mis-signals a live/failed/attention state as inert. An unknown kind still
 * falls back to the neutral gtag on purpose — the point is that *known* states
 * always carry their correct color. Color is confirmatory only; the label text
 * always carries the meaning too (see DESIGN_SYSTEM §9).
 */
const BADGE_KIND: Record<string, string> = {
  // ok — healthy / active / succeeded (green)
  ready: "ok",
  running: "ok",
  run: "ok",
  active: "ok",
  live: "ok",
  online: "ok",
  connected: "ok",
  done: "ok",
  complete: "ok",
  completed: "ok",
  success: "ok",
  succeeded: "ok",
  ok: "ok",
  healthy: "ok",
  passed: "ok",
  pass: "ok",
  // warn — transitional / needs attention (amber)
  warning: "warn",
  warn: "warn",
  pending: "warn",
  pend: "warn",
  queued: "warn",
  waiting: "warn",
  paused: "warn",
  idle: "warn",
  draining: "warn",
  degraded: "warn",
  stale: "warn",
  unknown: "warn",
  // err — failed / stopped / unreachable (red)
  blocked: "err",
  failed: "err",
  fail: "err",
  error: "err",
  offline: "err",
  disconnected: "err",
  stopped: "err",
  cancelled: "err",
  canceled: "err",
  killed: "err",
  crashed: "err",
  timeout: "err",
  "timed-out": "err",
};

export function Badge({ kind, children }: { kind?: string; children: ReactNode }) {
  const variant = kind ? (BADGE_KIND[kind.trim().toLowerCase()] ?? "") : "";
  return <span className={`gtag ${variant}`.trimEnd()}>{children}</span>;
}
