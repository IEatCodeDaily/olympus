/**
 * BoardPage — kanban board grid with columns by CardStatus.
 *
 * Renders STATUS_COLUMNS as CSS grid columns. Each column contains
 * KanbanCard items. Clicking a card fires onSelectCard.
 */

import type { Card, CardStatus } from "../../../types";
import { STATUS_COLUMNS } from "../../ProjectsView";
import { BoardColumn } from "../components/BoardColumn";

export function BoardPage({
  cards,
  selectedCardId,
  onSelectCard,
}: {
  cards: Card[];
  selectedCardId: string | null;
  onSelectCard: (id: string | null) => void;
}) {
  return (
    <div
      className="board-grid"
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${STATUS_COLUMNS.length}, minmax(200px, 1fr))`,
        gap: 10,
        alignItems: "start",
        height: "100%",
      }}
    >
      {STATUS_COLUMNS.map((col) => {
        const colCards = cards.filter((c) => c.status === col.key);
        return (
          <BoardColumn
            key={col.key}
            label={col.label}
            status={col.key}
            cards={colCards}
            selectedCardId={selectedCardId}
            onSelectCard={onSelectCard}
          />
        );
      })}
    </div>
  );
}
