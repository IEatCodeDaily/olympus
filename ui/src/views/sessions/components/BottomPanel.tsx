/**
 * BottomPanel — View-owned collapsible bottom panel.
 * Tabs: Terminal / Output / Debug.
 */

import { Icon } from "../../../components/Icon";

export type BpTab = "terminal" | "output" | "debug";

export function BottomPanel({
  height,
  tab,
  onTabChange,
  onClose,
}: {
  height?: number;
  tab: BpTab;
  onTabChange: (t: BpTab) => void;
  onClose: () => void;
}) {
  const tabs: Array<{ id: BpTab; label: string }> = [
    { id: "terminal", label: "Terminal" },
    { id: "output", label: "Output" },
    { id: "debug", label: "Debug" },
  ];

  return (
    <div className="bpanel" style={height ? { height } : undefined}>
      <div className="bp-tabs">
        <div className="bp-tablist">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`bp-tab${tab === t.id ? " on" : ""}`}
              onClick={() => onTabChange(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="bp-right">
          <button type="button" className="icobtn" title="Clear">
            <Icon name="trash" size={13} />
          </button>
          <button
            type="button"
            className="icobtn"
            title="Close panel"
            onClick={onClose}
          >
            <Icon name="chevron-down" size={13} />
          </button>
        </div>
      </div>
      <div className="bp-body">
        <div className="ln d">Coming soon…</div>
      </div>
    </div>
  );
}
