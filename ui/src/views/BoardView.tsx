import { useCallback, useEffect, useMemo, useState } from "react";
import {
  assignCard,
  blockCard,
  claimCard,
  completeCard,
  connectWs,
  createCard,
  fetchCards,
  onFrame,
  reassignCard,
} from "../api";
import { Badge, EmptyState, PageHeader, StatPill } from "../components/shell";
import { relativeTime } from "../lib/format";
import type { Card, CardStatus } from "../types";

type BoardColumn = "todo" | "ready" | "running" | "blocked" | "done";

const COLUMN_META: Record<BoardColumn, { label: string; hint: string }> = {
  todo: { label: "todo", hint: "New work waiting for assignment" },
  ready: { label: "ready", hint: "Assigned and queued" },
  running: { label: "running", hint: "Claimed by a worker" },
  blocked: { label: "blocked", hint: "Needs unblocking" },
  done: { label: "done", hint: "Completed cards" },
};

const COLUMN_ORDER: BoardColumn[] = ["todo", "ready", "running", "blocked", "done"];

function statusToColumn(status: CardStatus): BoardColumn {
  switch (status) {
    case "assigned":
      return "ready";
    case "claimed":
      return "running";
    default:
      return status;
  }
}

function isSessionLinked(card: Card): boolean {
  return Boolean(card.currentSessionId);
}

function openLinkedSession(sessionId: string): void {
  const escaped = typeof CSS !== "undefined" && typeof CSS.escape === "function"
    ? CSS.escape(sessionId)
    : sessionId.replace(/"/g, '\\"');

  const clickRow = () => {
    const row = document.querySelector(`.session-row[data-session-id="${escaped}"]`) as HTMLElement | null;
    if (!row) return false;
    row.click();
    return true;
  };

  const navButton = Array.from(document.querySelectorAll(".nav-item")).find((node) =>
    node.textContent?.trim() === "Sessions"
  ) as HTMLButtonElement | undefined;

  navButton?.click();
  if (clickRow()) return;

  let attempts = 0;
  const timer = window.setInterval(() => {
    attempts += 1;
    if (clickRow() || attempts > 20) {
      window.clearInterval(timer);
    }
  }, 150);
}

export default function BoardView() {
  const [boardId, setBoardId] = useState("default");
  const [search, setSearch] = useState("");
  const [draftTitle, setDraftTitle] = useState("");
  const [cards, setCards] = useState<Card[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedCardId, setSelectedCardId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [assignedId, setAssignedId] = useState("coding-agent");
  const [assignedKind, setAssignedKind] = useState("agent");
  const [sessionId, setSessionId] = useState("");
  const [attemptBookmark, setAttemptBookmark] = useState("");
  const [blockedBy, setBlockedBy] = useState("");

  const loadCards = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetchCards({ boardId: boardId.trim() || undefined });
      setCards(res.cards);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [boardId]);

  useEffect(() => {
    void loadCards();
  }, [loadCards]);

  useEffect(() => {
    connectWs();
    const off = onFrame((frame) => {
      if (frame.kind === "cards.changed") {
        void loadCards();
      }
    });
    const poll = window.setInterval(() => {
      void loadCards();
    }, 5000);
    return () => {
      off();
      window.clearInterval(poll);
    };
  }, [loadCards]);

  const filteredCards = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return cards;
    return cards.filter((card) => {
      return (
        card.title.toLowerCase().includes(q) ||
        (card.assignedId ?? "").toLowerCase().includes(q) ||
        (card.assignedKind ?? "").toLowerCase().includes(q) ||
        (card.currentSessionId ?? "").toLowerCase().includes(q)
      );
    });
  }, [cards, search]);

  const groupedCards = useMemo(() => {
    const groups: Record<BoardColumn, Card[]> = {
      todo: [],
      ready: [],
      running: [],
      blocked: [],
      done: [],
    };

    for (const card of filteredCards) {
      groups[statusToColumn(card.status)].push(card);
    }

    for (const column of COLUMN_ORDER) {
      groups[column].sort((a, b) => b.statusChangedAt - a.statusChangedAt || b.createdAt - a.createdAt);
    }

    return groups;
  }, [filteredCards]);

  const selectedCard = useMemo(
    () => cards.find((card) => card.id === selectedCardId) ?? null,
    [cards, selectedCardId]
  );

  useEffect(() => {
    if (!cards.length) {
      setSelectedCardId(null);
      return;
    }
    if (!selectedCardId || !cards.some((card) => card.id === selectedCardId)) {
      setSelectedCardId(cards[0].id);
    }
  }, [cards, selectedCardId]);

  useEffect(() => {
    if (!selectedCard) {
      setAssignedId("coding-agent");
      setAssignedKind("agent");
      setSessionId("");
      setAttemptBookmark("");
      setBlockedBy("");
      return;
    }
    setAssignedId(selectedCard.assignedId ?? "coding-agent");
    setAssignedKind(selectedCard.assignedKind ?? "agent");
    setSessionId(selectedCard.currentSessionId ?? "");
    setAttemptBookmark(selectedCard.currentBookmark ?? "");
    setBlockedBy(selectedCard.blockedBy.join(", "));
  }, [selectedCard]);

  const openCount = filteredCards.filter((card) => card.status !== "done").length;
  const runningCount = groupedCards.running.length;
  const blockedCount = groupedCards.blocked.length;

  const handleCreateCard = async () => {
    const title = draftTitle.trim();
    if (!title) return;
    setBusyAction("create");
    setActionError(null);
    try {
      const created = await createCard({ boardId: boardId.trim() || "default", title });
      setDraftTitle("");
      setSelectedCardId(created.id);
      await loadCards();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleAssign = async () => {
    if (!selectedCard) return;
    if (!assignedId.trim() || !assignedKind.trim() || !sessionId.trim() || !attemptBookmark.trim()) {
      setActionError("Assign requires assignee, kind, session id, and bookmark.");
      return;
    }

    setBusyAction("assign");
    setActionError(null);
    try {
      const updated = selectedCard.currentSessionId
        ? await reassignCard(selectedCard.id, {
            assignedId: assignedId.trim(),
            assignedKind: assignedKind.trim(),
            sessionId: sessionId.trim(),
            attemptBookmark: attemptBookmark.trim(),
            previousSessionId: selectedCard.currentSessionId,
          })
        : await assignCard(selectedCard.id, {
            assignedId: assignedId.trim(),
            assignedKind: assignedKind.trim(),
            sessionId: sessionId.trim(),
            attemptBookmark: attemptBookmark.trim(),
          });
      setSelectedCardId(updated.id);
      await loadCards();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleClaim = async () => {
    if (!selectedCard) return;
    setBusyAction("claim");
    setActionError(null);
    try {
      const updated = await claimCard(selectedCard.id);
      setSelectedCardId(updated.id);
      await loadCards();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleBlock = async () => {
    if (!selectedCard) return;
    const deps = blockedBy
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
    setBusyAction("block");
    setActionError(null);
    try {
      const updated = await blockCard(selectedCard.id, { blockedBy: deps });
      setSelectedCardId(updated.id);
      await loadCards();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleComplete = async () => {
    if (!selectedCard) return;
    setBusyAction("complete");
    setActionError(null);
    try {
      const updated = await completeCard(selectedCard.id);
      setSelectedCardId(updated.id);
      await loadCards();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div className="board-view view-scroll">
      <PageHeader
        title="Board"
        subtitle="Durable task board wired to live card events"
        actions={
          <div className="board-header-actions">
            <button type="button" className="board-ghost-btn" onClick={() => void loadCards()} disabled={loading || busyAction !== null}>
              Refresh
            </button>
            <button type="button" className="btn-primary" onClick={handleCreateCard} disabled={busyAction === "create" || !draftTitle.trim()}>
              {busyAction === "create" ? "Creating…" : "+ New card"}
            </button>
          </div>
        }
      />

      <div className="board-toolbar">
        <label className="board-field board-board-field">
          <span className="board-field-label">Board</span>
          <input
            value={boardId}
            onChange={(e) => setBoardId(e.target.value)}
            placeholder="default"
          />
        </label>
        <label className="board-field board-search-field">
          <span className="board-field-label">Filter</span>
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search title, assignee, or session"
          />
        </label>
        <label className="board-field board-create-field">
          <span className="board-field-label">New card title</span>
          <input
            value={draftTitle}
            onChange={(e) => setDraftTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void handleCreateCard();
            }}
            placeholder="Describe the task"
          />
        </label>
      </div>

      <div className="board-stats">
        <StatPill label="open" value={openCount} />
        <StatPill label="running" value={runningCount} />
        <StatPill label="blocked" value={blockedCount} />
      </div>

      {actionError && <div className="list-error">{actionError}</div>}
      {error && <div className="list-error">{error}</div>}

      {loading && cards.length === 0 ? (
        <BoardSkeleton />
      ) : (
        <div className="board-columns">
          {COLUMN_ORDER.map((column) => {
            const columnCards = groupedCards[column];
            return (
              <section key={column} className="board-column">
                <div className="board-column-head">
                  <div>
                    <div className="board-column-title">{COLUMN_META[column].label}</div>
                    <div className="board-column-hint">{COLUMN_META[column].hint}</div>
                  </div>
                  <span className="board-column-count">{columnCards.length}</span>
                </div>
                <div className="board-column-body">
                  {columnCards.length === 0 ? (
                    <EmptyState title="No cards" message={`Nothing in ${column}.`} />
                  ) : (
                    <div className="board-card-list">
                      {columnCards.map((card) => {
                        const selected = card.id === selectedCardId;
                        return (
                          <button type="button"
                            key={card.id}
                            className={`board-card ${selected ? "selected" : ""}`}
                            onClick={() => setSelectedCardId(card.id)}
                          >
                            <div className="board-card-head">
                              <Badge kind={column}>{column}</Badge>
                              <span className="board-card-age">{relativeTime(card.statusChangedAt)}</span>
                            </div>
                            <div className="board-card-title">{card.title}</div>
                            <div className="board-card-meta">
                              <span>{card.assignedId ?? "unassigned"}</span>
                              <span>{card.assignedKind ?? "queue"}</span>
                            </div>
                            <div className="board-card-meta board-card-meta-secondary">
                              <span>{card.currentSessionId ?? "no session"}</span>
                              <span>created {relativeTime(card.createdAt)}</span>
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              </section>
            );
          })}
        </div>
      )}

      <div className="board-detail">
        {!selectedCard ? (
          <EmptyState title="No card selected" message="Choose a card to inspect or mutate it." />
        ) : (
          <>
            <div className="board-detail-head">
              <div>
                <div className="board-detail-label">Selected card</div>
                <h2 className="board-detail-title">{selectedCard.title}</h2>
              </div>
              <div className="board-detail-badges">
                <Badge kind={statusToColumn(selectedCard.status)}>{statusToColumn(selectedCard.status)}</Badge>
                <Badge>{selectedCard.boardId}</Badge>
              </div>
            </div>

            <div className="board-detail-grid">
              <div className="board-detail-row">
                <span>Assignee</span>
                <span>{selectedCard.assignedId ?? "—"}</span>
              </div>
              <div className="board-detail-row">
                <span>Source</span>
                <span>{selectedCard.assignedKind ?? "—"}</span>
              </div>
              <div className="board-detail-row">
                <span>Linked session</span>
                <span>{selectedCard.currentSessionId ?? "—"}</span>
              </div>
              <div className="board-detail-row">
                <span>Blocked by</span>
                <span>{selectedCard.blockedBy.length ? selectedCard.blockedBy.join(", ") : "—"}</span>
              </div>
            </div>

            <div className="board-action-grid">
              <label className="board-field">
                <span className="board-field-label">Assignee</span>
                <input value={assignedId} onChange={(e) => setAssignedId(e.target.value)} placeholder="coding-agent" />
              </label>
              <label className="board-field">
                <span className="board-field-label">Kind</span>
                <input value={assignedKind} onChange={(e) => setAssignedKind(e.target.value)} placeholder="agent" />
              </label>
              <label className="board-field">
                <span className="board-field-label">Session id</span>
                <input value={sessionId} onChange={(e) => setSessionId(e.target.value)} placeholder="session-…" />
              </label>
              <label className="board-field">
                <span className="board-field-label">Bookmark</span>
                <input value={attemptBookmark} onChange={(e) => setAttemptBookmark(e.target.value)} placeholder="brief or handoff anchor" />
              </label>
              <label className="board-field board-field-wide">
                <span className="board-field-label">Blocked by</span>
                <input value={blockedBy} onChange={(e) => setBlockedBy(e.target.value)} placeholder="comma-separated dependency ids or notes" />
              </label>
            </div>

            <div className="board-detail-actions">
              <button type="button" className="btn-primary" onClick={() => void handleAssign()} disabled={busyAction !== null}>
                {busyAction === "assign"
                  ? (selectedCard.currentSessionId ? "Reassigning…" : "Assigning…")
                  : (selectedCard.currentSessionId ? "Reassign card" : "Assign card")}
              </button>
              <button type="button" className="board-ghost-btn" onClick={() => void handleClaim()} disabled={busyAction !== null || selectedCard.status === "claimed" || selectedCard.status === "done"}>
                {busyAction === "claim" ? "Claiming…" : "Claim card"}
              </button>
              <button type="button" className="board-ghost-btn" onClick={() => void handleBlock()} disabled={busyAction !== null || selectedCard.status === "done"}>
                {busyAction === "block" ? "Blocking…" : "Block card"}
              </button>
              <button type="button" className="board-ghost-btn" onClick={() => void handleComplete()} disabled={busyAction !== null || selectedCard.status === "done"}>
                {busyAction === "complete" ? "Completing…" : "Complete card"}
              </button>
              <button type="button"
                className="board-ghost-btn"
                onClick={() => {
                  if (selectedCard.currentSessionId) openLinkedSession(selectedCard.currentSessionId);
                }}
                disabled={!isSessionLinked(selectedCard)}
              >
                Open linked session
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function BoardSkeleton() {
  return (
    <div className="board-columns" aria-label="Loading board">
      {COLUMN_ORDER.map((column) => (
        <section key={column} className="board-column">
          <div className="board-column-head">
            <div>
              <div className="board-column-title">{COLUMN_META[column].label}</div>
              <div className="board-column-hint">{COLUMN_META[column].hint}</div>
            </div>
            <span className="board-column-count">—</span>
          </div>
          <div className="board-column-body">
            <div className="board-skeleton-list">
              {Array.from({ length: 3 }).map((_, index) => (
                <div key={index} className="board-skeleton-card">
                  <div className="board-skeleton-line board-skeleton-line-short" />
                  <div className="board-skeleton-line" />
                  <div className="board-skeleton-line board-skeleton-line-faint" />
                </div>
              ))}
            </div>
          </div>
        </section>
      ))}
    </div>
  );
}
