/**
 * Regression: consecutive drags must chain — the second drag starts from the
 * size left by the first. Before the sizeRef fix, onResizeStart captured a
 * stale `size` in its memoized closure, so every drag restarted from the
 * initial size and the panel snapped back (reported as "bottom panel resize
 * is not working").
 */
import { describe, it, expect } from "vitest";
import { render, act } from "@testing-library/react";
import { useResizable } from "./useResizable";

function Harness({ onState }: { onState: (s: ReturnType<typeof useResizable>) => void }) {
  const state = useResizable({
    axis: "y", min: 80, max: 400, initial: 152, direction: "down",
  });
  onState(state);
  return <div className="rz-y" onMouseDown={state.onResizeStart} />;
}

function drag(el: Element, fromY: number, toY: number) {
  act(() => {
    el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientY: fromY }));
  });
  act(() => {
    document.dispatchEvent(new MouseEvent("mousemove", { clientY: toY }));
  });
  act(() => {
    document.dispatchEvent(new MouseEvent("mouseup", {}));
  });
}

describe("useResizable", () => {
  it("chains consecutive drags instead of restarting from the initial size", () => {
    let latest: ReturnType<typeof useResizable> | null = null;
    const { container } = render(<Harness onState={(s) => (latest = s)} />);
    const bar = container.querySelector(".rz-y")!;

    // direction "down": dragging UP (negative delta) grows the panel.
    drag(bar, 300, 250); // +50 → 202
    expect(latest!.size).toBe(202);

    drag(bar, 300, 250); // +50 more → 252 (stale closure would give 202 again)
    expect(latest!.size).toBe(252);

    drag(bar, 300, 350); // drag down 50 → shrinks back to 202
    expect(latest!.size).toBe(202);
  });

  it("clamps to [min, max]", () => {
    let latest: ReturnType<typeof useResizable> | null = null;
    const { container } = render(<Harness onState={(s) => (latest = s)} />);
    const bar = container.querySelector(".rz-y")!;

    drag(bar, 300, 1000); // huge shrink → clamped to min
    expect(latest!.size).toBe(80);
    drag(bar, 1000, 0); // huge grow → clamped to max
    expect(latest!.size).toBe(400);
  });
});
