// Placeholder views for surfaces whose implementation card hasn't merged yet.
//
// Each renders a real .ol-* header + empty state — never a blank screen. When
// the real view card (V-UI / P1 / ST1) merges, it replaces these exports.

import { Icon, type IconName } from "../components/Icon";

export function ProjectsView() {
  return (
    <PlaceholderSurface
      icon="kanban"
      title="Projects"
      kicker="COMING · CARD P1"
      message="A durable kanban board: backlog, active, review, done — with agent assignment and per-card worker sessions."
    />
  );
}

export function SettingsView() {
  return (
    <PlaceholderSurface
      icon="gear"
      title="Settings"
      kicker="COMING · CARD ST1"
      message="Appearance (theme, density), model routing, and runtime configuration."
    />
  );
}

function PlaceholderSurface({
  icon,
  title,
  kicker,
  message,
}: {
  icon: IconName;
  title: string;
  kicker: string;
  message: string;
}) {
  return (
    <div className="shell-placeholder">
      <div className="gv-head">
        <Icon name={icon} size={14} />
        <span className="gv-title">{title}</span>
        <span className="gtag">{kicker}</span>
      </div>
      <div className="shell-placeholder-body">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name={icon} size={32} />
          </div>
          <div className="empty-state-title">{title}</div>
          <div className="empty-state-msg">{message}</div>
        </div>
      </div>
    </div>
  );
}
