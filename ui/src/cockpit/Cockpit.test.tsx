import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Cockpit } from "./Cockpit";
import { useCockpit } from "./store";

vi.mock("./tabs", () => ({
  getCockpitTabKind: () => ({ kind: "terminal", label: "Terminal", icon: "terminal", render: () => <div>live-runtime-marker</div> }),
  listCockpitTabKinds: () => [],
  UnknownKindPane: () => null,
}));

vi.mock("../api", () => ({ fetchTerminalTargets: vi.fn().mockResolvedValue([]) }));

describe("Cockpit visibility", () => {
  beforeEach(() => {
    useCockpit.setState({
      open: true,
      tabs: [{ id: "tab-a", kind: "terminal", title: "Hall 1", target: { nodeId: "hall" } }],
      activeTabId: "tab-a",
      geometry: { x: 120, y: 96, w: 820, h: 520 },
    });
  });

  it("keeps its single-pane runtime mounted while hidden", () => {
    const { container } = render(<Cockpit />);
    const marker = screen.getByText("live-runtime-marker");

    act(() => useCockpit.setState({ open: false }));

    expect(screen.getByText("live-runtime-marker")).toBe(marker);
    expect(container.querySelector(".cockpit")).toHaveClass("is-hidden");
    expect(container.querySelector(".cockpit")).toHaveAttribute("aria-hidden", "true");
  });
});
