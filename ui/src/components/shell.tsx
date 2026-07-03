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

/** Status badge. `kind` maps to the semantic .gtag color variants. */
const BADGE_KIND: Record<string, string> = {
  ready: "ok",
  running: "ok",
  done: "ok",
  online: "ok",
  warning: "warn",
  warn: "warn",
  blocked: "err",
  failed: "err",
  error: "err",
  offline: "err",
};

export function Badge({ kind, children }: { kind?: string; children: ReactNode }) {
  const variant = kind ? (BADGE_KIND[kind] ?? "") : "";
  return <span className={`gtag ${variant}`.trimEnd()}>{children}</span>;
}
