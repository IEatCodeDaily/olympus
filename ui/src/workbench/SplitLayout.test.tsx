import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createWorkbench, splitGroup } from "./model";
import { SplitLayout } from "./SplitLayout";

describe("SplitLayout", () => {
  it("renders nested groups and accessible keyboard-resizable separators", () => {
    let state = createWorkbench("group-a");
    state = splitGroup(state, "group-a", "horizontal", "split-a", "group-b");
    state = splitGroup(state, "group-b", "vertical", "split-b", "group-c");
    const onResize = vi.fn();

    render(
      <SplitLayout
        root={state.root}
        surfaceLabel="Session panes"
        renderGroup={(group) => <div>{group.id}</div>}
        onResize={onResize}
      />,
    );

    expect(screen.getByText("group-a")).toBeInTheDocument();
    expect(screen.getByText("group-b")).toBeInTheDocument();
    expect(screen.getByText("group-c")).toBeInTheDocument();
    const horizontal = screen.getByRole("separator", { name: "Resize Session panes left and right" });
    expect(horizontal).toHaveAttribute("aria-orientation", "vertical");
    expect(horizontal).toHaveAttribute("aria-valuemin", "20");
    expect(horizontal).toHaveAttribute("aria-valuemax", "80");
    fireEvent.keyDown(horizontal, { key: "ArrowRight" });
    expect(onResize).toHaveBeenCalledWith("split-a", 55);
  });
});
