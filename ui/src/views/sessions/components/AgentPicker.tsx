/**
 * AgentPicker — modal for selecting which agent (profile) to start a
 * new session with.
 *
 * Bug 10: "New Session" opens an agent picker (list from /api/agents);
 * selecting an agent creates the managed session bound to that profile.
 * Bug 3: shows id + provider only (no "ACP/CLI" dual-label).
 *
 * Uses the fixed-position .ol-overlay + .ol-dialog so it floats above
 * any scrollable content (consistent with ForkModal / bug 1).
 */

import { Icon } from "../../../components/Icon";
import { BrandIcon, agentBrand } from "../../../components/BrandIcons";
import { useAgents } from "../../../hooks/queries";

export function AgentPicker({
  open,
  onSelect,
  onCancel,
}: {
  open: boolean;
  onSelect: (agentId: string) => void;
  onCancel: () => void;
}) {
  const { data: agentsData, isLoading } = useAgents();
  const agents = agentsData?.agents ?? [];

  if (!open) return null;

  return (
    <div
      className="ol-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Start new session"
      onClick={onCancel}
    >
      <div
        className="ol-dialog"
        style={{ maxWidth: 400, width: "90vw" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="ol-dialog-head">
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="plus" size={18} />
            <div>
              <div className="ol-dialog-title">New session</div>
            </div>
          </div>
          <button
            type="button"
            className="icobtn"
            onClick={onCancel}
            title="Close"
            aria-label="Close"
          >
            <Icon name="x" size={14} />
          </button>
        </div>
        <div className="ol-dialog-body" style={{ padding: 0 }}>
          <div className="gk" style={{ padding: "8px 12px 4px" }}>
            choose an agent
          </div>
          {isLoading && (
            <div style={{ padding: "8px 12px", color: "var(--dim)", fontSize: 12 }}>
              Loading agents…
            </div>
          )}
          {!isLoading && agents.length === 0 && (
            <div style={{ padding: "8px 12px", color: "var(--dim)", fontSize: 12 }}>
              No agents configured. Start a default session instead.
            </div>
          )}
          <div style={{ maxHeight: 300, overflowY: "auto" }}>
            {agents.map((a) => (
              <button
                key={a.id}
                type="button"
                className="ol-menu-item"
                style={{
                  width: "100%",
                  border: "none",
                  background: "none",
                  cursor: "pointer",
                  textAlign: "left",
                }}
                onClick={() => onSelect(a.id)}
              >
                <BrandIcon name={agentBrand(a.kind, a.provider)} size={15} />
                <span style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
                    {a.id}
                    {a.isDefault && (
                      <span
                        className="tag"
                        style={{
                          marginLeft: 6,
                          fontSize: 9,
                          color: "var(--silver)",
                          background: "var(--silver-wash)",
                        }}
                      >
                        default
                      </span>
                    )}
                    {a.ready === false && (
                      <span
                        className="tag"
                        style={{
                          marginLeft: 6,
                          fontSize: 9,
                          color: "var(--warn, orange)",
                        }}
                        title="CLI installed but no credentials found — log in first"
                      >
                        needs login
                      </span>
                    )}
                  </div>
                  <div style={{ fontSize: 10, color: "var(--faint)", fontFamily: "var(--font-mono)" }}>
                    {/* Bug 3: id + provider only, no ACP/CLI dual-label */}
                    {a.provider ?? "—"}
                    {a.model ? ` · ${a.model}` : ""}
                  </div>
                </span>
              </button>
            ))}
          </div>
        </div>
        <div className="ol-dialog-foot">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
