/**
 * KanbanCard — a single card inside a kanban column.
 *
 * Shows title, assignee badge, status tag, and age.
 * Uses existing CSS tokens: .kcard, .gtag, .grow.
 */

import type { Card } from "../../../types";
import { timeAgo } from "../../sessions/helpers";
import { statusBadgeClass } from "./statusUtils";

/** Status → display label mapping. */
const STATUS_LABELS: Record<string, string> = {
  todo: "todo",
  assigned: "run",
  claimed: "run",
  blocked: "blocked",
  done: "done",
};

export function KanbanCard({
  card,
  isSelected,
  onClick,
}: {
  card: Card;
  isSelected: boolean;
  onClick: () => void;
}) {
  const badgeClass = statusBadgeClass(card.status);
  const statusLabel = STATUS_LABELS[card.status] ?? card.status;

  return (
    <div
      className={`kcard${isSelected ? " kcard-sel" : ""}`}
      onClick={onClick}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <div style={{ fontSize: 12.5, marginBottom: 6, lineHeight: 1.3 }}>
        {card.title}
      </div>
      <div className="grow">
        {card.assignedId ? (
          <span className="gk">{card.assignedId}</span>
        ) : (
          <span className="gk">unassigned</span>
        )}
        <span className={`gtag ${badgeClass}`}>{statusLabel}</span>
      </div>
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          color: "var(--faint)",
          marginTop: 4,
        }}
      >
        {timeAgo(card.createdAt)}
      </div>
    </div>
  );
}
