// UsageView — budget / token / subscription spend (roadmap U3). Backend /api/usage
// lands with Epic I. Placeholder shows the metric + per-model breakdown layout.
import { PageHeader, PlaceholderBadge, StatPill } from "../components/shell";

const MODELS = ["gpt-5.4", "claude-sonnet-4-6", "glm-5.2"];

export default function UsageView() {
  return (
    <div className="view-scroll">
      <PageHeader
        title="Usage"
        subtitle="Token spend, budget, and subscription limits per model / provider"
        actions={<PlaceholderBadge epic="Epic I (budget)" />}
      />
      <div className="board-stats">
        <StatPill label="tokens (24h)" value="—" />
        <StatPill label="est. cost (24h)" value="—" />
        <StatPill label="active models" value={String(MODELS.length)} />
      </div>
      <div className="usage-list">
        {MODELS.map((m) => (
          <div key={m} className="usage-row">
            <span className="usage-model">{m}</span>
            <div className="usage-bar"><div className="usage-bar-fill" style={{ width: "0%" }} /></div>
            <span className="usage-val">— tok</span>
          </div>
        ))}
      </div>
    </div>
  );
}
