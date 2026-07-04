/**
 * ForkModal — the fork-confirmation dialog.
 *
 * Bug 1: uses fixed-position .ol-overlay + .ol-dialog so it floats above
 * a long scrollable transcript instead of rendering inline.
 */

import { Icon } from "../../../components/Icon";

export function ForkModal({
  open,
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  if (!open) return null;

  return (
    <div
      className="ol-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onClick={onCancel}
    >
      <div
        className="ol-dialog"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="ol-dialog-head">
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="git-branch" size={18} />
            <div>
              <div className="ol-dialog-title">{title}</div>
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
        <div className="ol-dialog-body">
          {message}
        </div>
        <div className="ol-dialog-foot">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="btn pri" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
