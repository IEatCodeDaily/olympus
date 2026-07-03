/**
 * ProjectsView — the Projects View component (owns sidebar + kanban board).
 *
 * Architecture (mirrors SessionsView pattern):
 *   View OWNS:
 *     - left sidebar (board list + assignee filter) — ProjectSidebar
 *     - viewport layout (board grid + right detail panel)
 *     - right detail panel — CardDetailPanel
 *
 * Pages own viewport content ONLY:
 *   - BoardPage (kanban columns by CardStatus)
 *
 * Routes (URL-persistent):
 *   /projects              → BoardPage (all cards)
 *   /projects/$boardId    → BoardPage (filtered to board)
 *
 *   ┌──────────────────────────────────────────────────────────────┐
 *   │ gv-head (title · board badge · actions)                     │
 *   ├──────────────────────────────────────────────────────────────┤
 *   │ gv-body                                                      │
 *   │   board-grid (columns by status)  │ detail panel             │
 *   │                                  │ (sticky aside)            │
 *   └──────────────────────────────────────────────────────────────┘
 */

import React, { useState, useMemo } from "react";
import { useUIStore } from "../store";
import { useCards } from "../hooks/queries";
import type { Card, CardStatus } from "../types";
import { timeAgo } from "./sessions/helpers";
import { Icon } from "../components/Icon";

import { ProjectSidebar } from "./projects/components/ProjectSidebar";
import { BoardPage } from "./projects/pages/BoardPage";
import { CardDetailPanel } from "./projects/components/CardDetailPanel";

/** All status columns in display order. */
export const STATUS_COLUMNS: { key: CardStatus; label: string }[] = [
  { key: "todo", label: "Todo" },
  { key: "assigned", label: "Assigned" },
  { key: "claimed", label: "Claimed" },
  { key: "blocked", label: "Blocked" },
  { key: "done", label: "Done" },
];

/** Extract unique assignee IDs from a card list. */
function uniqueAssignees(cards: Card[]): string[] {
  const set = new Set<string>();
  for (const c of cards) {
    if (c.assignedId) set.add(c.assignedId);
  }
  return Array.from(set).sort();
}

export function ProjectsView() {
  const { sidebarCollapsed } = useUIStore();
  const [selectedCardId, setSelectedCardId] = useState<string | null>(null);
  const [filterAssignee, setFilterAssignee] = useState<string | null>(null);

  const { data, isLoading } = useCards();

  const cards: Card[] = data?.cards ?? [];
  const assignees = useMemo(() => uniqueAssignees(cards), [cards]);

  const filteredCards = useMemo(() => {
    if (!filterAssignee) return cards;
    return cards.filter((c) => c.assignedId === filterAssignee);
  }, [cards, filterAssignee]);

  const selectedCard = selectedCardId
    ? cards.find((c) => c.id === selectedCardId) ?? null
    : null;

  return (
    <>
      {/* ── View-owned left sidebar ─────────────────────────────── */}
      {!sidebarCollapsed && (
        <ProjectSidebar
          assignees={assignees}
          activeFilter={filterAssignee}
          onFilterChange={setFilterAssignee}
          cards={cards}
        />
      )}

      {/* ── Viewport layout ─────────────────────────────────────── */}
      <div className="viewport">
        <div
          className="view on"
          data-view="projects"
          style={{ flexDirection: "row" }}
        >
          {/* Board area — flexes to fill available space */}
          <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
            {/* Header */}
            <div className="gv-head">
              <span className="gv-title">Projects</span>
              <span className="gtag">{cards.length} cards</span>
              <div className="sp" />
              {filterAssignee && (
                <button
                  type="button"
                  className="gtag ok"
                  style={{ cursor: "pointer" }}
                  onClick={() => setFilterAssignee(null)}
                  title="Clear filter"
                >
                  {filterAssignee} ×
                </button>
              )}
            </div>

            {/* Board body */}
            <div className="gv-body">
              {isLoading ? (
                <div className="empty-state">
                  <div className="empty-state-msg">Loading…</div>
                </div>
              ) : filteredCards.length === 0 ? (
                <div className="empty-state">
                  <div className="empty-state-icon">
                    <Icon name="kanban" size={32} />
                  </div>
                  <div className="empty-state-title">No cards</div>
                  <div className="empty-state-msg">
                    {cards.length === 0
                      ? "Cards will appear here when agents pick up work."
                      : "No cards match the current filter."}
                  </div>
                </div>
              ) : (
                <BoardPage
                  cards={filteredCards}
                  selectedCardId={selectedCardId}
                  onSelectCard={setSelectedCardId}
                />
              )}
            </div>
          </div>

          {/* Right detail panel */}
          {selectedCard && (
            <>
              <div className="rz-x" />
              <CardDetailPanel
                card={selectedCard}
                onClose={() => setSelectedCardId(null)}
              />
            </>
          )}
        </div>
      </div>
    </>
  );
}
