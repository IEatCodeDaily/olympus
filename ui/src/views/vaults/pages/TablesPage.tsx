// TablesPage — vault tables view (stub for this milestone).
//
// The backend doesn't emit structured tables yet (Epic K). This renders a
// real .ol-* placeholder panel so the VIEWS toggle is functional.

import { Icon } from "../../../components/Icon";

export function TablesPage() {
  return (
    <div className="vault-content">
      <div className="empty-state">
        <div className="empty-state-icon">
          <Icon name="layout-grid" size={32} />
        </div>
        <div className="empty-state-title">Tables</div>
        <div className="empty-state-msg">
          Structured data views from this vault's notes. Coming in Epic K.
        </div>
      </div>
    </div>
  );
}
