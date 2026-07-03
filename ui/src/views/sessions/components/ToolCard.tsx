/**
 * ToolCard — a collapsible tool-call dropdown.
 *
 * Bug 8: renders as a collapsible dropdown (icon + toolName + status),
 * expandable to show input parameters (args) and output (result).
 */

import React from "react";
import { Icon } from "../../../components/Icon";
import type { ToolCall } from "../../../types";

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
  const done = tc.result != null;
  const statusIcon = done ? "check" : "clock";
  const statusColor = done ? "var(--green)" : "var(--amber)";

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
        <Icon name={statusIcon} size={12} style={{ color: statusColor } as React.CSSProperties} />
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
