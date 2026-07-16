import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Cockpit } from "./Cockpit";
import { useCockpit } from "./store";

const fetchTerminalTargets = vi.hoisted(() =>
  vi.fn().mockResolvedValue([{ id: "hall", label: "Hall", kind: "hall", default: true }]),
);

vi.mock("./tabs", () => {
  const kinds = [
    { kind: "terminal", label: "Terminal", icon: "terminal", needsNode: true },
    { kind: "browser", label: "Browser", icon: "globe", needsNode: false },
    { kind: "editor", label: "Code editor", icon: "file", needsNode: true },
  ];
  return {
    getCockpitTabKind: (kind: string) => ({
      ...(kinds.find((k) => k.kind === kind) ?? kinds[0]),
      render: () => <div>live-runtime-marker</div>,
    }),
    listCockpitTabKinds: () => kinds,
    UnknownKindPane: () => null,
  };
});

vi.mock("../api", () => ({ fetchTerminalTargets }));

describe("Cockpit visibility", () => {
  beforeEach(() => {
    fetchTerminalTargets.mockClear();
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

  it("opens the new-tab menu from the titlebar while tabs exist", () => {
    render(<Cockpit />);

    fireEvent.click(screen.getByTitle("New tab"));

    expect(screen.getByRole("menu", { name: "" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Terminal/ })).toBeInTheDocument();
  });

  it("opens node submenus on hover", async () => {
    render(<Cockpit />);

    fireEvent.click(screen.getByTitle("New tab"));
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /Terminal/ }));

    await waitFor(() => expect(screen.getByRole("menuitem", { name: /Hall/ })).toBeInTheDocument());
    expect(fetchTerminalTargets).toHaveBeenCalledTimes(1);
  });
});
