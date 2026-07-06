/**
 * ToolCard — a collapsible tool-call dropdown with lifecycle status.
 *
 * Status rendering:
 *  - pending:     amber clock  — awaiting permission / queued
 *  - in_progress: amber spinner — running
 *  - completed:   green check
 *  - failed:      red x
 */

import React from "react";
import { Icon } from "../../../components/Icon";
import type { ToolCall } from "../../../types";

function StatusBadge({ status }: { status: string }) {
  const cls = `tc-status tc-${status}`;
  const label =
    status === "pending"
      ? "waiting"
      : status === "in_progress"
        ? "running"
        : status === "failed"
          ? "failed"
          : status;
  return (
    <span className={cls}>
      {status === "pending" && <Icon name="clock" size={10} />}
      {status === "in_progress" && <span className="tc-spin" />}
      {status === "completed" && <Icon name="check" size={10} />}
      {status === "failed" && <Icon name="x" size={10} />}
      {label}
    </span>
  );
}

export function ToolCard({
  tc,
  idx,
  expanded,
  onToggle,
}: {
  tc: ToolCall;
  idx: number;
  expanded: boolean;
  onToggle: (idx: number) => void;
}) {
  const status = tc.status ?? (tc.result != null ? "completed" : "pending");

  const argsStr = React.useMemo(() => {
    if (!tc.args) return "";
    if (typeof tc.args === "string") return tc.args;
    try {
      return JSON.stringify(tc.args, null, 2);
    } catch {
      return String(tc.args);
    }
  }, [tc.args]);

  return (
    <div className="toolcard muted">
      <div
        className="tc-head"
        style={{ cursor: "pointer" }}
        onClick={() => onToggle(idx)}
      >
        <StatusBadge status={status} />
        <span className="nm">{tc.label ?? tc.name}</span>
        <span className="sp" />
        <Icon name={expanded ? "chevron-down" : "chevron-right"} size={12} />
      </div>
      {expanded && (
        <div className="tc-body">
          {argsStr && (
            <>
              <div className="gk" style={{ marginBottom: 3 }}>args</div>
              <div className="tc-out" style={{ marginBottom: 6 }}>
                {argsStr}
              </div>
            </>
          )}
          {tc.result != null && (
            <>
              <div className="gk" style={{ marginBottom: 3 }}>output</div>
              <div className="tc-out">{tc.result}</div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
