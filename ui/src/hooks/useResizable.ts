/**
 * useResizable — generic drag-to-resize hook.
 *
 * Attach a `mousedown` handler (from this hook) to a `.rz-x` or `.rz-y` bar
 * element. On mousedown, it captures the starting position and initial size,
 * then tracks `mousemove` to update the size in pixels, clamped to [min, max].
 *
 * Axis:
 *   "x" — horizontal drag (resizes width); delta = currentX - startX
 *   "y" — vertical drag   (resizes height); delta = currentY - startY
 *
 * direction:
 *   "right" — panel is to the LEFT of the bar; dragging right DECREASES width
 *             (used for: left sidebar ↔ viewport — the bar is on the right
 *             edge of the sidebar)
 *   "left"  — panel is to the RIGHT of the bar; dragging left DECREASES width
 *             (used for: viewport ↔ right sidebar — the bar is on the left
 *             edge of the right sidebar)
 *   "down"  — panel is ABOVE the bar; dragging down DECREASES height
 *             (used for: chatcol ↔ bottom panel — the bar is on the bottom
 *             edge of the chatcol)
 *
 * Usage:
 *   const { size, onResizeStart } = useResizable({
 *     axis: "x", min: 160, max: 400, initial: 220, direction: "right",
 *   });
 *   <div className="rz-x" onMouseDown={onResizeStart} />
 *   <aside style={{ width: size }} />
 */

import { useState, useCallback, useRef, useEffect } from "react";

export interface UseResizableOptions {
  axis: "x" | "y";
  min: number;
  max: number;
  initial: number;
  /** Direction the panel grows relative to the drag bar. */
  direction: "right" | "left" | "down";
  /** Persist to localStorage under this key (optional). */
  persistKey?: string;
}

export function useResizable(opts: UseResizableOptions) {
  const { axis, min, max, direction, persistKey } = opts;

  // Load persisted size if provided
  const loadInitial = useCallback((): number => {
    if (persistKey) {
      try {
        const stored = localStorage.getItem(persistKey);
        if (stored) {
          const n = parseInt(stored, 10);
          if (!isNaN(n)) return Math.max(min, Math.min(max, n));
        }
      } catch {
        // ignore
      }
    }
    return opts.initial;
  }, [persistKey, min, max, opts.initial]);

  const [size, setSize] = useState<number>(loadInitial);
  const startRef = useRef<{ pos: number; size: number } | null>(null);
  // Live mirror of `size` so onResizeStart never captures a stale closure.
  // Without this, the memoized callback (deps: axis/direction/clamp) keeps the
  // size from its creation render, and every drag after the first restarts
  // from that stale value — the panel snaps back and resize appears broken.
  const sizeRef = useRef(size);
  sizeRef.current = size;

  const clamp = useCallback(
    (n: number) => Math.max(min, Math.min(max, n)),
    [min, max],
  );

  const onResizeStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const pos = axis === "x" ? e.clientX : e.clientY;
      startRef.current = { pos, size: sizeRef.current };

      const onMove = (ev: MouseEvent) => {
        if (!startRef.current) return;
        const current = axis === "x" ? ev.clientX : ev.clientY;
        const delta = current - startRef.current.pos;

        let next: number;
        if (direction === "right") {
          // Panel on the left; dragging right increases width
          next = startRef.current.size + delta;
        } else if (direction === "left") {
          // Panel on the right; dragging left increases width
          next = startRef.current.size - delta;
        } else {
          // "down": panel is above; dragging down decreases height
          next = startRef.current.size - delta;
        }
        next = clamp(next);
        setSize(next);
      };

      const onUp = () => {
        startRef.current = null;
        document.removeEventListener("mousemove", onMove);
        document.removeEventListener("mouseup", onUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.addEventListener("mousemove", onMove);
      document.addEventListener("mouseup", onUp);
      document.body.style.cursor = axis === "x" ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
    },
    [axis, direction, clamp],
  );

  // Persist on change
  useEffect(() => {
    if (!persistKey) return;
    try {
      localStorage.setItem(persistKey, String(size));
    } catch {
      // ignore
    }
  }, [size, persistKey]);

  return { size, setSize, onResizeStart };
}
