// NodesView — fleet management (roadmap U4). Backend /api/nodes lands with Epic L.
// Placeholder shows the node-grid layout; build mock-first then flip to real.
import { PageHeader, EmptyState, PlaceholderBadge, StatPill } from "../components/shell";

export default function NodesView() {
  return (
    <div className="view-scroll">
      <PageHeader
        title="Nodes"
        subtitle="Fleet — registered nodes, heartbeat, available slots"
        actions={<PlaceholderBadge epic="Epic L (multi-node)" />}
      />
      <div className="board-stats">
        <StatPill label="nodes" value="1" />
        <StatPill label="slots free" value="—" />
        <StatPill label="online" value="1" />
      </div>
      <div className="node-grid">
        <div className="node-card">
          <div className="node-card-head">
            <span className="node-name">local</span>
            <span className="badge badge-running">online</span>
          </div>
          <div className="node-card-body">
            <div className="node-stat"><span>slots</span><span>— / —</span></div>
            <div className="node-stat"><span>heartbeat</span><span>just now</span></div>
            <div className="node-stat"><span>runtime</span><span>hermes-acp</span></div>
          </div>
        </div>
      </div>
      <EmptyState
        title="Single-node MVP"
        message="Multi-node fleet management arrives with Epic L (iroh transport). This view will list every connected node, its slots, and health."
      />
    </div>
  );
}
