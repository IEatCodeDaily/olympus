import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Message } from "../../../types";
import { BottomPanel } from "./BottomPanel";

const messagesBySession = new Map<string, Message[]>();

vi.mock("../../../hooks/queries", () => ({
  useMessages: (sessionId: string | null) => ({
    data: { messages: sessionId ? (messagesBySession.get(sessionId) ?? []) : [] },
  }),
}));

vi.mock("../../../api", () => ({
  onFrame: () => () => undefined,
}));

function systemMessage(sessionId: string, messageId: number, content: string): Message {
  return {
    messageId,
    sessionId,
    role: "system",
    content,
    toolName: null,
    toolCalls: null,
    reasoning: null,
    timestamp: messageId,
    tokenCount: null,
    finishReason: "error",
  };
}

describe("BottomPanel Hall-backed logs", () => {
  it("rehydrates retained Hall history after switching A to B to A", () => {
    messagesBySession.set("session-a", [systemMessage("session-a", 1, "A retained failure")]);
    messagesBySession.set("session-b", [systemMessage("session-b", 2, "B retained failure")]);

    const props = {
      height: 200,
      tab: "logs" as const,
      onTabChange: vi.fn(),
      onClose: vi.fn(),
    };
    const view = render(<BottomPanel {...props} sessionId="session-a" />);
    expect(screen.getByText("A retained failure")).toBeInTheDocument();

    view.rerender(<BottomPanel {...props} sessionId="session-b" />);
    expect(screen.getByText("B retained failure")).toBeInTheDocument();
    expect(screen.queryByText("A retained failure")).not.toBeInTheDocument();

    view.rerender(<BottomPanel {...props} sessionId="session-a" />);
    expect(screen.getByText("A retained failure")).toBeInTheDocument();
    expect(screen.queryByText("B retained failure")).not.toBeInTheDocument();
  });
});
