import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import React from "react";
import { getCockpitTabKind } from "./tabs";
import { useCockpit, type CockpitTab } from "./store";

// ── Mocks ───────────────────────────────────────────────────────────────

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    loadAddon() {}
    open() {}
    write() {}
    focus() {}
    dispose() {}
    onData() {
      return { dispose: () => {} };
    }
    onResize() {
      return { dispose: () => {} };
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

vi.mock("../api", () => ({
  terminalWsUrl: () => "ws://test/terminal",
}));

vi.mock("../components/Icon", () => ({
  Icon: () => null,
}));

// ── Fake WebSocket ──────────────────────────────────────────────────────

class FakeWebSocket {
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  static instances: FakeWebSocket[] = [];
  readyState = FakeWebSocket.OPEN;
  close = vi.fn((code?: number) => {
    this.readyState = FakeWebSocket.CLOSED;
    setTimeout(() => {
      this.onclose?.({ code: code ?? 1000 } as CloseEvent);
    }, 0);
  });
  send = vi.fn();
  onopen: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  constructor(_url: string) {
    FakeWebSocket.instances.push(this);
  }
}

function makeTab(id: string): CockpitTab {
  return {
    id,
    kind: "terminal",
    title: "Hall 1",
    target: { nodeId: "hall" },
  };
}

function renderTerminalPane(tab: CockpitTab, visible = true) {
  const def = getCockpitTabKind("terminal");
  if (!def) throw new Error("terminal tab kind not registered");
  return render(<div>{def.render({ tab, visible })}</div>);
}

// ── Tests ───────────────────────────────────────────────────────────────

describe("TerminalPane reconnect behavior", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    useCockpit.setState({
      open: true,
      tabs: [makeTab("term-reconnect")],
      activeTabId: "term-reconnect",
      geometry: { x: 0, y: 0, w: 820, h: 520 },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("schedules a reconnect after an abnormal socket close", () => {
    const tab = makeTab("term-reconnect");
    const { unmount } = renderTerminalPane(tab);

    expect(FakeWebSocket.instances).toHaveLength(1);
    const sock1 = FakeWebSocket.instances[0];

    act(() => {
      sock1.readyState = FakeWebSocket.CLOSED;
      sock1.onclose?.({ code: 1006 } as CloseEvent);
    });

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(FakeWebSocket.instances).toHaveLength(2);
    unmount();
  });

  it("does NOT reconnect after explicit tab close (unmount)", () => {
    const tab = makeTab("term-reconnect-2");
    const { unmount } = renderTerminalPane(tab);

    const beforeCount = FakeWebSocket.instances.length;
    unmount();

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(FakeWebSocket.instances).toHaveLength(beforeCount);
  });

  it("does NOT reconnect when server sends 'exited' message", () => {
    const tab = makeTab("term-reconnect-3");
    const { unmount } = renderTerminalPane(tab);

    const sock = FakeWebSocket.instances[FakeWebSocket.instances.length - 1];

    act(() => {
      sock.onmessage?.({
        data: JSON.stringify({ kind: "exited", exitCode: 0 }),
      } as MessageEvent);
    });

    act(() => {
      sock.readyState = FakeWebSocket.CLOSED;
      sock.onclose?.({ code: 1000 } as CloseEvent);
    });

    const countAfterClose = FakeWebSocket.instances.length;

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(FakeWebSocket.instances).toHaveLength(countAfterClose);
    unmount();
  });
});
