import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Cockpit } from "./Cockpit";
import { useCockpit } from "./store";

const terminalDispose = vi.hoisted(() => vi.fn());

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    loadAddon() {}
    open() {}
    write() {}
    focus() {}
    dispose = terminalDispose;
    onData() { return { dispose: vi.fn() }; }
    onResize() { return { dispose: vi.fn() }; }
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));
vi.mock("../api", () => ({
  fetchTerminalTargets: vi.fn().mockResolvedValue([]),
  terminalWsUrl: () => "ws://example.test/terminal",
}));

class FakeWebSocket {
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];
  readyState = FakeWebSocket.OPEN;
  close = vi.fn();
  send = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(_url: string) { FakeWebSocket.instances.push(this); }
}

describe("Cockpit terminal lifetime", () => {
  beforeEach(() => {
    FakeWebSocket.instances = [];
    terminalDispose.mockClear();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    useCockpit.setState({
      open: true,
      tabs: [{ id: "terminal-a", kind: "terminal", title: "Hall 1", target: { nodeId: "hall" } }],
      activeTabId: "terminal-a",
      geometry: { x: 120, y: 96, w: 820, h: 520 },
    });
  });

  it("does not close or dispose the live terminal when the single-pane cockpit is hidden", () => {
    render(<Cockpit />);
    expect(FakeWebSocket.instances).toHaveLength(1);
    const socket = FakeWebSocket.instances[0];

    act(() => useCockpit.setState({ open: false }));

    expect(socket.close).not.toHaveBeenCalled();
    expect(terminalDispose).not.toHaveBeenCalled();
  });
});
