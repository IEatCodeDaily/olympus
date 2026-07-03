/**
 * BoardColumn — single kanban column (status lane).
 *
 * Uses existing CSS tokens: .col, .col-h, .col-b, .kcard.
 */

import type { Card, CardStatus } from "../../../types";
import { KanbanCard } from "./KanbanCard";

export function BoardColumn({
  label,
  status,
  cards,
  selectedCardId,
  onSelectCard,
}: {
  label: string;
  status: CardStatus;
  cards: Card[];
  selectedCardId: string | null;
  onSelectCard: (id: string | null) => void;
}) {
  return (
    <div className="col" data-col={status}>
      <div className="col-h">
        <span>{label}</span>
        <span className="colct">{cards.length}</span>
      </div>
      <div className="col-b">
        {cards.length === 0 && (
          <div
            style={{
              color: "var(--faint)",
              fontSize: 11,
              textAlign: "center",
              padding: "12px 0",
            }}
          >
            —
          </div>
        )}
        {cards.map((card) => (
          <KanbanCard
            key={card.id}
            card={card}
            isSelected={card.id === selectedCardId}
            onClick={() => onSelectCard(card.id === selectedCardId ? null : card.id)}
          />
        ))}
      </div>
    </div>
  );
}
