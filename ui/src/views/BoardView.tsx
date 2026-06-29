// BoardView — kanban board (roadmap U1). Backend /api/cards is LIVE (C1 merged),
// so this placeholder shows the real column structure; a worker wires it to the
// API. Columns mirror card status; cards are 1:1 with worker sessions.
import { PageHeader, EmptyState, StatPill } from "../components/shell";

const COLUMNS = ["todo", "ready", "running", "blocked", "done"] as const;

export default function BoardView() {
  return (
    <div className="view-scroll">
      <PageHeader
        title="Board"
        subtitle="Kanban — durable tasks, 1:1 with worker sessions"
        actions={<button className="btn-primary" disabled>+ New card</button>}
      />
      <div className="board-stats">
        <StatPill label="open" value="—" />
        <StatPill label="running" value="—" />
        <StatPill label="blocked" value="—" />
      </div>
      <div className="board-columns">
        {COLUMNS.map((col) => (
          <div key={col} className="board-column">
            <div className="board-column-head">
              <span className="board-column-title">{col}</span>
              <span className="board-column-count">0</span>
            </div>
            <div className="board-column-body">
              <EmptyState title="No cards" message={`Nothing in ${col}.`} />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
