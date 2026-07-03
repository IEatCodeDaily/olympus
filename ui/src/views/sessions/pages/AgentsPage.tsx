/**
 * AgentsPage — viewport content for the "Agents" NavItem.
 *
 * Bug 4: Fetches /api/agents — shows all drivable profiles with their
 * provider + model. Bug 3: shows id + provider only (no ACP/CLI dual-label).
 *
 * This is Page content — it renders inside the View's viewport slot only.
 */

import { Icon } from "../../../components/Icon";
import { useAgents } from "../../../hooks/queries";
import type { AgentInfo } from "../../../types";

export function AgentsPage() {
  const { data: agentsData, isLoading } = useAgents();
  const agents = agentsData?.agents ?? [];

  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Agents</span>
        <span className="sp" />
        <span className="ct">{agents.length}</span>
      </div>
      <div className="gv-body">
        {isLoading && (
          <div className="empty-state-msg" style={{ padding: 16 }}>
            Loading agents…
          </div>
        )}
        {!isLoading && agents.length === 0 && (
          <div className="empty-state">
            <div className="empty-state-icon">
              <Icon name="bot" size={32} />
            </div>
            <div className="empty-state-title">No agents configured</div>
            <div className="empty-state-msg">
              Agents are Hermes profiles. Configure them in your Hermes
              profile directory to make them appear here.
            </div>
          </div>
        )}
        <div className="agent-grid">
          {agents.map((a) => (
            <AgentCard key={a.id} agent={a} />
          ))}
        </div>
      </div>
    </>
  );
}

function AgentCard({ agent }: { agent: AgentInfo }) {
  return (
    <div className="ol-card agent-card">
      <div className="agent-card-head">
        <Icon name="bot" size={16} />
        <span className="agent-name">{agent.id}</span>
        {agent.isDefault && (
          <span
            className="tag"
            style={{
              color: "var(--silver)",
              background: "var(--silver-wash)",
              fontSize: 9,
            }}
          >
            default
          </span>
        )}
      </div>
      <div className="kv">
        <span className="k">PROVIDER</span>
        <span className="v">{agent.provider ?? "—"}</span>
      </div>
      <div className="kv">
        <span className="k">MODEL</span>
        <span className="v" style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>
          {agent.model ?? "—"}
        </span>
      </div>
    </div>
  );
}
