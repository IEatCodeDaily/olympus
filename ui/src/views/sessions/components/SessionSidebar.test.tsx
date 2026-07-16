import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SessionSidebar } from "./SessionSidebar";
import type { Session } from "../../../types";

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
  useRouterState: () => ({ location: { pathname: "/sessions/s-1" } }),
}));

vi.mock("../../../hooks/queries", () => ({
  useSessions: () => ({
    data: {
      sessions: [
        {
          id: "s-1",
          hermesId: "h-1",
          orgId: "personal",
          ownerId: "rpw",
          contextId: null,
          source: "olympus",
          title: "Focused session",
          startedAt: Math.floor(Date.now() / 1000),
          lastActivity: Math.floor(Date.now() / 1000),
          messageCount: 0,
          inputTokens: 0,
          outputTokens: 0,
          liveness: "idle",
          managed: true,
          pinned: false,
          archived: false,
          forkedFrom: null,
          forkPoint: null,
          forkType: null,
          agent: null,
          model: null,
          node: null,
          capabilities: null,
        } satisfies Session,
      ],
    },
  }),
  useUpdateSession: () => ({ mutate: vi.fn() }),
}));

vi.mock("../../../api", () => ({ createSession: vi.fn() }));

describe("SessionSidebar", () => {
  it("marks the active session as open and focused", () => {
    const { container } = render(<SessionSidebar width={220} activeSessionId="s-1" openSessionIds={new Set(["s-1"])} />);

    const row = container.querySelector("[data-session-id='s-1']");
    expect(row).toHaveAttribute("data-open", "true");
    expect(row).toHaveAttribute("data-focused", "true");
    expect(row).toHaveClass("focused");
  });

  it("keeps an open, unfocused session subtly highlighted", () => {
    const { container } = render(<SessionSidebar width={220} activeSessionId={null} openSessionIds={new Set(["s-1"])} />);

    const row = container.querySelector("[data-session-id='s-1']");
    expect(row).toHaveAttribute("data-open", "true");
    expect(row).toHaveAttribute("data-focused", "false");
    expect(row).not.toHaveClass("focused");
  });

  it("writes a dockview session drag payload", () => {
    render(<SessionSidebar width={220} activeSessionId="s-1" />);
    const data = new Map<string, string>();

    fireEvent.dragStart(screen.getByText("Focused session").closest(".srow") as HTMLElement, {
      dataTransfer: {
        effectAllowed: "none",
        setData: (type: string, value: string) => data.set(type, value),
      },
    });

    expect(JSON.parse(data.get("application/x-olympus-session") ?? "{}")).toMatchObject({
      type: "session",
      sessionId: "s-1",
    });
  });
});
