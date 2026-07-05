/**
 * QueuePanel — stacked queue cards floating above the composer (zcode-style).
 *
 * Messages typed while a turn is running are queued here instead of being
 * sent. Each card offers:
 *  - drag handle for reordering (HTML5 DnD)
 *  - inline edit (pencil → textarea → Enter/blur saves)
 *  - Steer: inject THIS item into the running turn right now (removes it
 *    from the queue)
 *  - delete
 *
 * The queue itself lives in ChatPage (persisted per-session); this is a
 * controlled presentational component.
 */

import React, { useRef, useState } from "react";
import { Icon } from "../../../components/Icon";

export interface QueuedMsg {
  id: string;
  text: string;
}

export function QueuePanel({
  items,
  onReorder,
  onEdit,
  onDelete,
  onSteer,
}: {
  items: QueuedMsg[];
  onReorder: (from: number, to: number) => void;
  onEdit: (id: string, text: string) => void;
  onDelete: (id: string) => void;
  onSteer: (id: string) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState("");
  const dragFrom = useRef<number | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);

  if (items.length === 0) return null;

  const startEdit = (m: QueuedMsg) => {
    setEditingId(m.id);
    setEditText(m.text);
  };

  const commitEdit = () => {
    if (editingId != null) {
      const t = editText.trim();
      if (t) onEdit(editingId, t);
      else onDelete(editingId);
    }
    setEditingId(null);
  };

  return (
    <div className="queue-panel" role="list" aria-label="Queued messages">
      {items.map((m, i) => (
        <div
          key={m.id}
          role="listitem"
          className={`queue-card${dragOver === i ? " drag-over" : ""}`}
          draggable={editingId !== m.id}
          onDragStart={(e) => {
            dragFrom.current = i;
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(i);
          }}
          onDragLeave={() => setDragOver((v) => (v === i ? null : v))}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(null);
            if (dragFrom.current != null && dragFrom.current !== i) {
              onReorder(dragFrom.current, i);
            }
            dragFrom.current = null;
          }}
          onDragEnd={() => {
            dragFrom.current = null;
            setDragOver(null);
          }}
        >
          <span className="qc-grip" title="Drag to reorder" aria-hidden>
            ⋮⋮
          </span>
          {editingId === m.id ? (
            <textarea
              className="qc-edit"
              rows={1}
              value={editText}
              autoFocus
              onChange={(e) => setEditText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  commitEdit();
                }
                if (e.key === "Escape") setEditingId(null);
              }}
              onBlur={commitEdit}
            />
          ) : (
            <span className="qc-text" title={m.text}>
              {m.text}
            </span>
          )}
          <div className="qc-actions">
            <button
              type="button"
              className="qc-steer"
              title="Send now as a steer into the running turn"
              onClick={() => onSteer(m.id)}
            >
              <Icon name="arrow-up" size={11} />
              <span>Steer</span>
            </button>
            <button
              type="button"
              className="qc-btn"
              title="Edit"
              aria-label="Edit queued message"
              onClick={() => startEdit(m)}
            >
              <Icon name="pencil" size={12} />
            </button>
            <button
              type="button"
              className="qc-btn"
              title="Delete"
              aria-label="Delete queued message"
              onClick={() => onDelete(m.id)}
            >
              <Icon name="trash" size={12} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
