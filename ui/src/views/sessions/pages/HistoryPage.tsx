/**
 * HistoryPage — viewport content for the "History" NavItem.
 *
 * The full session archive: EVERY session from every agent/channel (managed
 * AND observed, archived included via toggle), filterable by:
 *   - node       (session.node)
 *   - agent      (session.agent)
 *   - channel    (session.source: cli/telegram/discord/webui/cron/…)
 *   - time range (last hour / day / week / month / all)
 *   - archived   (hidden by default)
 * Free-text filter matches title/agent/model.
 *
 * The sidebar RECENT section only shows the 5 most recent sessions — this
 * page is where the rest live.
 *
 * This is Page content — renders inside the View's viewport slot only.
 */

import { useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import { useSessions, useUpdateSession } from "../../../hooks/queries";
import type { Session } from "../../../types";
import { timeAgo } from "../helpers";

type TimeRange = "all" | "1h" | "24h" | "7d" | "30d";

/** Rows rendered at once — "Show more" reveals the next batch (keeps the DOM sane with 1800+ sessions). */
const PAGE_SIZE = 100;

const TIME_RANGES: { id: TimeRange; label: string; secs: number | null }[] = [
  { id: "all", label: "All time", secs: null },
  { id: "1h", label: "Last hour", secs: 3600 },
  { id: "24h", label: "Last 24h", secs: 86400 },
  { id: "7d", label: "Last 7 days", secs: 7 * 86400 },
  { id: "30d", label: "Last 30 days", secs: 30 * 86400 },
];

export function HistoryPage() {
  const navigate = useNavigate();
  // All sessions — managed and observed. Archived visibility is a toggle.
  const [showArchived, setShowArchived] = useState(false);
  const { data } = useSessions(
    showArchived ? {} : { archived: false },
  );
  const sessions = useMemo(() => data?.sessions ?? [], [data]);
  const update = useUpdateSession();

  const [node, setNode] = useState<string>("");
  const [agent, setAgent] = useState<string>("");
  const [channel, setChannel] = useState<string>("");
  const [range, setRange] = useState<TimeRange>("all");
  const [q, setQ] = useState<string>("");
  const [visible, setVisible] = useState(PAGE_SIZE);

  // Distinct filter options derived from the data itself.
  const nodes = useMemo(
    () => distinct(sessions.map((s) => s.node)),
    [sessions],
  );
  const agents = useMemo(
    () => distinct(sessions.map((s) => s.agent)),
    [sessions],
  );
  const channels = useMemo(
    () => distinct(sessions.map((s) => s.source)),
    [sessions],
  );

  const filtered = useMemo(() => {
    const cutoff = TIME_RANGES.find((t) => t.id === range)?.secs ?? null;
    const now = Date.now() / 1000;
    const needle = q.trim().toLowerCase();
    return sessions.filter((s) => {
      if (node && s.node !== node) return false;
      if (agent && s.agent !== agent) return false;
      if (channel && s.source !== channel) return false;
      if (cutoff !== null && now - s.lastActivity > cutoff) return false;
      if (needle) {
        const hay = `${s.title ?? ""} ${s.agent ?? ""} ${s.model ?? ""}`.toLowerCase();
        if (!hay.includes(needle)) return false;
      }
      return true;
    });
  }, [sessions, node, agent, channel, range, q]);

  return (
    <>
      <div className="gv-head">
        <span className="gv-title">History</span>
        <span className="sp" />
        <span className="gk">
          {filtered.length} of {sessions.length} sessions
        </span>
      </div>
      <div className="gv-body">
        {/* Filter bar */}
        <div
          className="hist-filters"
          style={{ display: "flex", gap: 8, flexWrap: "wrap", padding: "0 0 12px", alignItems: "center" }}
        >
          <input
            type="search"
            className="hist-search"
            placeholder="Filter by title, agent, model…"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            style={{ minWidth: 200 }}
          />
          <FilterSelect label="Node" value={node} onChange={setNode} options={nodes} />
          <FilterSelect label="Agent" value={agent} onChange={setAgent} options={agents} />
          <FilterSelect label="Channel" value={channel} onChange={setChannel} options={channels} />
          <select
            className="hist-select"
            value={range}
            onChange={(e) => setRange(e.target.value as TimeRange)}
            title="Time range"
          >
            {TIME_RANGES.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </select>
          <label
            style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "var(--dim)", cursor: "pointer" }}
          >
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(e) => setShowArchived(e.target.checked)}
            />
            Show archived
          </label>
        </div>

        {/* Session rows */}
        {filtered.length === 0 ? (
          <div className="empty-state">
            <div className="empty-state-icon">
              <Icon name="clock" size={32} />
            </div>
            <div className="empty-state-title">No sessions match</div>
            <div className="empty-state-msg">Adjust the filters above.</div>
          </div>
        ) : (
          <div className="hist-list">
            {filtered.slice(0, visible).map((s) => (
              <HistoryRow
                key={s.id}
                session={s}
                onOpen={() =>
                  void navigate({ to: "/sessions/$sessionId", params: { sessionId: s.id } })
                }
                onTogglePin={() =>
                  update.mutate({ id: s.id, patch: { pinned: !s.pinned } })
                }
                onToggleArchive={() =>
                  update.mutate({ id: s.id, patch: { archived: !s.archived } })
                }
              />
            ))}
            {filtered.length > visible && (
              <button
                type="button"
                className="btn"
                style={{ margin: "8px auto", display: "block" }}
                onClick={() => setVisible((v) => v + PAGE_SIZE)}
              >
                Show more ({filtered.length - visible} remaining)
              </button>
            )}
          </div>
        )}
      </div>
    </>
  );
}

function distinct(values: (string | null | undefined)[]): string[] {
  return Array.from(new Set(values.filter((v): v is string => !!v))).sort();
}

function FilterSelect({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: string[];
}) {
  return (
    <select
      className="hist-select"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      title={label}
    >
      <option value="">{`All ${label.toLowerCase()}s`}</option>
      {options.map((o) => (
        <option key={o} value={o}>
          {o}
        </option>
      ))}
    </select>
  );
}

function HistoryRow({
  session,
  onOpen,
  onTogglePin,
  onToggleArchive,
}: {
  session: Session;
  onOpen: () => void;
  onTogglePin: () => void;
  onToggleArchive: () => void;
}) {
  return (
    <div
      className="ol-card hist-row"
      data-session-id={session.id}
      style={{ display: "flex", alignItems: "center", gap: 10, cursor: "pointer", marginBottom: 6 }}
      onClick={onOpen}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{ fontSize: 12, fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
        >
          {session.pinned && <Icon name="pin" size={10} />} {session.title || "Untitled"}
        </div>
        <div style={{ fontSize: 10, color: "var(--faint)", fontFamily: "var(--font-mono)", display: "flex", gap: 8 }}>
          <span>{session.source}</span>
          {session.agent && <span>{session.agent}</span>}
          {session.node && <span>@{session.node}</span>}
          {session.model && <span>{session.model}</span>}
          <span>{session.messageCount} msgs</span>
          {session.archived && <span style={{ color: "var(--warn, orange)" }}>archived</span>}
        </div>
      </div>
      <span style={{ fontSize: 10, color: "var(--faint)", whiteSpace: "nowrap" }}>
        {timeAgo(session.lastActivity)}
      </span>
      <span className="srow-actions" style={{ display: "flex", gap: 4 }}>
        <button
          type="button"
          className="srow-act"
          title={session.pinned ? "Unpin" : "Pin"}
          onClick={(e) => {
            e.stopPropagation();
            onTogglePin();
          }}
        >
          <Icon name="pin" size={11} />
        </button>
        <button
          type="button"
          className="srow-act"
          title={session.archived ? "Unarchive" : "Archive"}
          onClick={(e) => {
            e.stopPropagation();
            onToggleArchive();
          }}
        >
          <Icon name="archive" size={11} />
        </button>
      </span>
    </div>
  );
}
