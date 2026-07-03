/**
 * CardDetailPanel — right-side sticky aside showing full card details.
 *
 * Displays:
 *   - Title + status badge
 *   - Metadata (ID, board, priority, timestamps)
 *   - Assignee info
 *   - Body (markdown via react-markdown) if available
 *   - Blocked-by list
 *   - Attempts history
 */

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Card } from "../../../types";
import { timeAgo } from "../../sessions/helpers";
import { Icon } from "../../../components/Icon";
import { statusBadgeClass } from "./statusUtils";

export function CardDetailPanel({
  card,
  onClose,
}: {
  card: Card;
  onClose: () => void;
}) {
  const badgeClass = statusBadgeClass(card.status);

  return (
    <aside className="rsidebar" style={{ width: 340, overflowY: "auto", background: "var(--chrome)", borderLeft: "var(--border-w) solid var(--border)" }}>
      {/* Panel header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "10px 12px",
          borderBottom: "var(--border-w) solid var(--border)",
        }}
      >
        <span style={{ fontWeight: 600, flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {card.title}
        </span>
        <span className={`gtag ${badgeClass}`}>{card.status}</span>
        <button
          type="button"
          className="icobtn"
          onClick={onClose}
          title="Close"
        >
          <Icon name="x" size={14} />
        </button>
      </div>

      {/* Details */}
      <div style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 12 }}>
        {/* Meta row */}
        <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 10px", fontSize: 11 }}>
          <span style={{ color: "var(--faint)", fontFamily: "var(--font-mono)" }}>ID</span>
          <span style={{ fontFamily: "var(--font-mono)", wordBreak: "break-all" }}>{card.id}</span>

          <span style={{ color: "var(--faint)", fontFamily: "var(--font-mono)" }}>BOARD</span>
          <span>{card.boardId}</span>

          <span style={{ color: "var(--faint)", fontFamily: "var(--font-mono)" }}>PRIORITY</span>
          <span>{card.priority}</span>

          <span style={{ color: "var(--faint)", fontFamily: "var(--font-mono)" }}>CREATED</span>
          <span>{new Date(card.createdAt * 1000).toLocaleString()}</span>

          <span style={{ color: "var(--faint)", fontFamily: "var(--font-mono)" }}>CHANGED</span>
          <span>{timeAgo(card.statusChangedAt)} ago</span>
        </div>

        {/* Assignee */}
        {card.assignedId && (
          <div style={{ padding: "8px 10px", background: "var(--elev)", borderRadius: "var(--radius)", border: "var(--border-w) solid var(--border)" }}>
            <div style={{ fontSize: 9, fontFamily: "var(--font-mono)", color: "var(--faint)", marginBottom: 4 }}>ASSIGNED TO</div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span className="gk">{card.assignedId}</span>
              {card.assignedKind && (
                <span className="gtag">{card.assignedKind}</span>
              )}
            </div>
            {card.currentSessionId && (
              <div style={{ fontSize: 10, color: "var(--dim)", marginTop: 4, fontFamily: "var(--font-mono)" }}>
                session: {card.currentSessionId.slice(0, 12)}…
              </div>
            )}
          </div>
        )}

        {/* Blocked by */}
        {card.blockedBy.length > 0 && (
          <div>
            <div style={{ fontSize: 9, fontFamily: "var(--font-mono)", color: "var(--faint)", marginBottom: 4 }}>BLOCKED BY</div>
            {card.blockedBy.map((dep) => (
              <span key={dep} className="gtag warn" style={{ marginRight: 4, marginBottom: 4 }}>{dep}</span>
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}
