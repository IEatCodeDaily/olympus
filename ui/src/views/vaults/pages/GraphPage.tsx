// GraphPage — vault link graph view (stub for this milestone).
//
// A full interactive graph (nodes = notes, edges = wikilinks) is Epic K/L.
// This renders a real .ol-* placeholder so the VIEWS toggle is functional.

import { Icon } from "../../../components/Icon";

export function GraphPage() {
  return (
    <div className="vault-content">
      <div className="empty-state">
        <div className="empty-state-icon">
          <Icon name="workflow" size={32} />
        </div>
        <div className="empty-state-title">Link graph</div>
        <div className="empty-state-msg">
          Visualize how notes connect through wikilinks. Coming in Epic K.
        </div>
      </div>
    </div>
  );
}
