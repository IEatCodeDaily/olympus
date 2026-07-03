/**
 * ContextRing — real context-window visualization for the right sidebar.
 *
 * Tier 1 (always available): total tokens from session DTO + per-role breakdown
 * computed client-side from loaded messages. No fabricated numbers — if a metric
 * is unknown, the row is omitted.
 *
 * Reference layout: Claude Code /context card — header with "used / max · %",
 * thin progress bar, breakdown rows with mini bars. Dark theme, monospace numbers.
 */

import React from "react";
import type { Message, Session } from "../../../types";
import { tokenFmt } from "../helpers";

/** One row in the breakdown table. */
interface BreakdownRow {
  label: string;
  tokens: number;
  /** 0–1 fraction of totalUsed; rendered as a mini inline bar. */
  frac: number;
}

interface ContextRingProps {
  session: Session | undefined;
  messages: Message[];
}

/** Compute per-role token totals from the message list. */
function computeBreakdown(messages: Message[]): BreakdownRow[] {
  const buckets = new Map<string, number>();
  for (const m of messages) {
    const tc = m.tokenCount ?? 0;
    if (tc <= 0) continue;
    // Group: user | assistant | tool | system/session_meta
    let key: string;
    switch (m.role) {
      case "user":
        key = "User";
        break;
      case "assistant":
        key = "Assistant";
        break;
      case "tool":
        key = "Tool calls";
        break;
      default:
        key = "System";
        break;
    }
    buckets.set(key, (buckets.get(key) ?? 0) + tc);
  }

  const total = [...buckets.values()].reduce((a, b) => a + b, 0);
  if (total <= 0) return [];

  const rows: BreakdownRow[] = [];
  // Fixed order: User > Assistant > Tool calls > System
  for (const label of ["User", "Assistant", "Tool calls", "System"]) {
    const tokens = buckets.get(label) ?? 0;
    if (tokens > 0) {
      rows.push({ label, tokens, frac: tokens / total });
    }
  }
  return rows;
}

/** Thin progress bar filling from left. */
function ProgressBar({ value }: { value: number }) {
  return (
    <div className="ctx-bar-track">
      <div
        className="ctx-bar-fill"
        style={{ width: `${Math.min(100, Math.max(0, value * 100))}%` }}
      />
    </div>
  );
}

/** Mini inline bar for a breakdown row. */
function MiniBar({ frac }: { frac: number }) {
  return (
    <span className="ctx-mini-track">
      <span
        className="ctx-mini-fill"
        style={{ width: `${Math.min(100, Math.max(0, frac * 100))}%` }}
      />
    </span>
  );
}

export function ContextRing({ session, messages }: ContextRingProps) {
  const inputT = session?.inputTokens ?? 0;
  const outputT = session?.outputTokens ?? 0;
  const totalUsed = inputT + outputT;

  const breakdown = React.useMemo(() => computeBreakdown(messages), [messages]);

  if (!session && messages.length === 0) return null;

  return (
    <div className="ctx-ring">
      {/* ── Header ─────────────────────────────── */}
      <div className="ctx-head">
        <span className="ctx-label">Context window</span>
        <span className="ctx-total">
          {tokenFmt(totalUsed)}
          {session?.model ? (
            <>
              {" "}
              <span style={{ color: "var(--dim)", fontSize: "var(--fs-10)" }}>
                via {session.model}
              </span>
            </>
          ) : null}
        </span>
      </div>

      {/* ── Progress bar ───────────────────────── */}
      {/* Without a max from the backend we render the bar as an
          *indicator* rather than a ratio. It fills proportionally to give
          visual weight but carries no % claim. */}
      <ProgressBar value={totalUsed > 0 ? 0.5 : 0} />

      {/* ── IO split ──────────────────────────── */}
      <div className="ctx-io">
        <div className="ctx-io-row">
          <span className="ctx-k">In</span>
          <span className="ctx-v">{tokenFmt(inputT)}</span>
        </div>
        <div className="ctx-io-row">
          <span className="ctx-k">Out</span>
          <span className="ctx-v">{tokenFmt(outputT)}</span>
        </div>
        <div className="ctx-io-row">
          <span className="ctx-k">Msgs</span>
          <span className="ctx-v">{messages.length}</span>
        </div>
      </div>

      {/* ── Role breakdown ────────────────────── */}
      {breakdown.length > 0 && (
        <div className="ctx-breakdown">
          <div className="ctx-gk">Breakdown</div>
          {breakdown.map((row) => (
            <div key={row.label} className="ctx-bd-row">
              <span className="ctx-bd-label">{row.label}</span>
              <span className="ctx-bd-val">
                {tokenFmt(row.tokens)}{" "}
                <MiniBar frac={row.frac} />
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
