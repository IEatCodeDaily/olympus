/**
 * UsagePage — viewport content for the "Usage" NavItem.
 *
 * Shows per-agent token usage + cost estimates. Backend endpoint
 * (/api/usage) doesn't exist yet, so this renders a typed placeholder
 * using the UsageSummary shape from types.ts. The layout flips to real
 * data with no structural change once the endpoint ships.
 *
 * This is Page content — renders inside the View's viewport slot only.
 */

import { Icon } from "../../../components/Icon";
import { tokenFmt } from "../helpers";

// FIXTURE: usage summaries — backend /api/usage not shipped yet.
const MOCK_USAGE = [
  { model: "claude-opus-4-8", provider: "anthropic", tokensIn: 1240000, tokensOut: 312000, estCost: 42.5 },
  { model: "gpt-5.4", provider: "openai-codex", tokensIn: 890000, tokensOut: 201000, estCost: 28.1 },
  { model: "glm-5.2", provider: "zai", tokensIn: 450000, tokensOut: 98000, estCost: 6.3 },
];

export function UsagePage() {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Usage</span>
        <span className="sp" />
      </div>
      <div className="gv-body">
        <div className="gk" style={{ padding: "0 0 8px" }}>
          last 30 days · fixture
        </div>
        <div className="usage-list">
          {MOCK_USAGE.map((u, i) => (
            <div key={i} className="ol-card usage-row">
              <div className="usage-model">
                <Icon name="bot" size={14} />
                <span style={{ fontWeight: 500 }}>{u.model}</span>
                <span style={{ color: "var(--faint)", fontSize: 10, fontFamily: "var(--font-mono)" }}>
                  {u.provider}
                </span>
              </div>
              <div className="stats">
                <div className="stat">
                  <span className="v">{tokenFmt(u.tokensIn)}</span>
                  <span className="l">IN</span>
                </div>
                <div className="stat">
                  <span className="v">{tokenFmt(u.tokensOut)}</span>
                  <span className="l">OUT</span>
                </div>
                <div className="stat">
                  <span className="v" style={{ color: "var(--silver)" }}>
                    ${u.estCost.toFixed(1)}
                  </span>
                  <span className="l">COST</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
