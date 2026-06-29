// Shared shell primitives for Olympus views. View workers build on these so
// every screen feels consistent. All styling via CSS-variable classes (themeable).
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
    <div className="page-header">
      <div className="page-header-text">
        <h1 className="page-title">{title}</h1>
        {subtitle && <p className="page-subtitle">{subtitle}</p>}
      </div>
      {actions && <div className="page-header-actions">{actions}</div>}
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
      {message && <div className="empty-state-message">{message}</div>}
      {cta && <div className="empty-state-cta">{cta}</div>}
    </div>
  );
}

/** "Coming soon" placeholder banner for views whose backend epic isn't live. */
export function PlaceholderBadge({ epic }: { epic: string }) {
  return (
    <span className="placeholder-badge" title={`Backend: ${epic}`}>
      placeholder · {epic}
    </span>
  );
}

/** A metric chip. */
export function StatPill({ label, value }: { label: string; value: ReactNode }) {
  return (
    <span className="stat-pill">
      <span className="stat-pill-value">{value}</span>
      <span className="stat-pill-label">{label}</span>
    </span>
  );
}

/** Status badge (todo/running/blocked/done, online/offline, etc.). */
export function Badge({ kind, children }: { kind?: string; children: ReactNode }) {
  return <span className={`badge ${kind ? `badge-${kind}` : ""}`}>{children}</span>;
}
