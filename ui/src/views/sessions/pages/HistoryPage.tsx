/**
 * HistoryPage — the full session archive, styled as a proper data table
 * (shadcn Table/Select/Input patterns rendered with the Instrument .ol-*
 * design tokens — this repo does not use Tailwind).
 *
 * EVERY session from every agent/channel, filterable by node, agent,
 * channel, time range; free text matches title/agent/model; archived
 * hidden behind a toggle. Sidebar RECENT shows only 5 — this is the rest.
 */

import { useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import { useSessions, useUpdateSession } from "../../../hooks/queries";
import type { Session } from "../../../types";
import { timeAgo } from "../helpers";

type TimeRange = "all" | "1h" | "24h" | "7d" | "30d";

/** Rows rendered at once — "Show more" reveals the next batch. */
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
  const [showArchived, setShowArchived] = useState(false);
  const { data } = useSessions(showArchived ? {} : { archived: false });
  const sessions = useMemo(() => data?.sessions ?? [], [data]);
  const update = useUpdateSession();

  const [node, setNode] = useState<string>("");
  const [agent, setAgent] = useState<string>("");
  const [channel, setChannel] = useState<string>("");
  const [range, setRange] = useState<TimeRange>("all");
  const [q, setQ] = useState<string>("");
  const [visible, setVisible] = useState(PAGE_SIZE);

  const nodes = useMemo(() => distinct(sessions.map((s) => s.node)), [sessions]);
  const agents = useMemo(() => distinct(sessions.map((s) => s.agent)), [sessions]);
  const channels = useMemo(() => distinct(sessions.map((s) => s.source)), [sessions]);

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
        {/* ── Filter toolbar ── */}
        <div className="hist-filters">
          <div className="hist-search-wrap">
            <Icon name="search" size={13} />
            <input
              type="search"
              className="hist-search"
              placeholder="Filter by title, agent, model…"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
          </div>
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
          <label className="hist-archived-toggle">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(e) => setShowArchived(e.target.checked)}
            />
            <span>Archived</span>
          </label>
        </div>

        {/* ── Data table ── */}
        {filtered.length === 0 ? (
          <div className="empty-state">
            <div className="empty-state-icon">
              <Icon name="clock" size={32} />
            </div>
            <div className="empty-state-title">No sessions match</div>
            <div className="empty-state-msg">Adjust the filters above.</div>
          </div>
        ) : (
          <div className="hist-table-wrap">
            <table className="hist-table">
              <thead>
                <tr>
                  <th className="col-title">Session</th>
                  <th className="col-channel">Channel</th>
                  <th className="col-agent">Agent</th>
                  <th className="col-node">Node</th>
                  <th className="col-msgs">Msgs</th>
                  <th className="col-time">Activity</th>
                  <th className="col-acts" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
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
              </tbody>
            </table>
            {filtered.length > visible && (
              <button
                type="button"
                className="btn hist-more"
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
    <tr
      className="hist-row"
      data-session-id={session.id}
      data-managed={session.managed ? "true" : "false"}
      onClick={onOpen}
    >
      <td className="col-title">
        <span className="hist-title">
          {session.pinned && <Icon name="pin" size={10} />}
          {session.title || "Untitled"}
        </span>
        {session.archived && <span className="gtag warn hist-archived-tag">archived</span>}
      </td>
      <td className="col-channel">
        <span className="gtag">{session.source}</span>
      </td>
      <td className="col-agent mono">{session.agent ?? "—"}</td>
      <td className="col-node mono">{session.node ?? "—"}</td>
      <td className="col-msgs mono">{session.messageCount}</td>
      <td className="col-time mono">{timeAgo(session.lastActivity)}</td>
      <td className="col-acts">
        <span className="hist-acts">
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
      </td>
    </tr>
  );
}
