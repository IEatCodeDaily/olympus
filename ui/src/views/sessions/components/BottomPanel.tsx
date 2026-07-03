/**
 * BottomPanel — View-owned collapsible bottom panel.
 * Tabs: Terminal / Output / Debug.
 */

import { Icon } from "../../../components/Icon";
import { MOCK_TERMINAL_LINES } from "../fixtures";

export type BpTab = "terminal" | "output" | "debug";

export function BottomPanel({
  tab,
  onTabChange,
  onClose,
}: {
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
    <div className="bpanel">
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
        {tab === "terminal" &&
          MOCK_TERMINAL_LINES.map((l, i) => (
            <div key={i} className={`ln ${l.cls}`}>{l.text}</div>
          ))}
        {tab === "output" && <div className="ln d">No output yet.</div>}
        {tab === "debug" && <div className="ln d">No debug data.</div>}
      </div>
    </div>
  );
}
