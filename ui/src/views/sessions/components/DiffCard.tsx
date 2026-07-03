/**
 * DiffCard — a collapsible unified-diff rendering of a tool call result.
 * Detects patch/write_file/edit_file or result strings containing @@ markers.
 */

import React, { useState } from "react";
import { Icon } from "../../../components/Icon";
import type { ToolCall } from "../../../types";
import { parseDiff } from "../helpers";

export function DiffCard({ tc }: { tc: ToolCall }) {
  const result = tc.result ?? "";
  const args = tc.args as Record<string, unknown> | null;
  const filePath =
    typeof args === "object" && args && typeof args.path === "string"
      ? args.path
      : tc.name;
  const lines = parseDiff(result);
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="toolcard">
      <div
        className="tc-head"
        style={{ cursor: "pointer" }}
        onClick={() => setCollapsed((v) => !v)}
      >
        <Icon name="check" size={12} style={{ color: "var(--green)" } as React.CSSProperties} />
        <span className="nm">{tc.name} · {filePath}</span>
        <span className="sp" />
        <Icon name={collapsed ? "chevron-right" : "chevron-down"} size={12} />
      </div>
      {!collapsed && (
        <div className="tc-body">
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, marginBottom: 4, color: "var(--faint)" }}>
            {filePath}
          </div>
          {lines.map((l, i) => (
            <div
              key={i}
              className={`diffln${l.type === "add" ? " add" : l.type === "del" ? " del" : ""}`}
              style={l.type === "hdr" ? { color: "var(--dim)", opacity: 0.7 } : undefined}
            >
              {l.text}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
