import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import React from "react";
import type { ReactNode } from "react";
import type { Message, Session } from "../types";

// ── Mocks ──────────────────────────────────────────
// Mock TanStack Router's useNavigate
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
}));

// Mock the query hooks
const mockSession: Partial<Session> = {
  id: "test-sess",
  title: "Test Session",
  agent: "test-agent",
  model: "test-model",
  liveness: "idle",
  messageCount: 2,
};

const mockMessages: Message[] = [
  {
    messageId: 0,
    sessionId: "test-sess",
    role: "user",
    content: "Hello, world!",
    timestamp: 1700000000,
    toolCalls: null,
    toolName: null,
    reasoning: null,
    tokenCount: null,
    finishReason: null,
  },
  {
    messageId: 1,
    sessionId: "test-sess",
    role: "assistant",
    content: "Hi there! How can I help?",
    timestamp: 1700000001,
    toolCalls: null,
    toolName: null,
    reasoning: null,
    tokenCount: null,
    finishReason: null,
  },
];

vi.mock("../hooks/queries", () => ({
  useSession: () => ({ data: mockSession }),
  useMessages: () => ({ data: { messages: mockMessages }, isLoading: false }),
}));

// Mock the API
const mockSendMessage = vi.fn().mockResolvedValue(undefined);
vi.mock("../api", () => ({
  sendMessage: (...args: unknown[]) => mockSendMessage(...args),
  onFrame: () => () => {}, // no-op unsubscribe
}));

// ── Tests ──────────────────────────────────────────
describe("ChatView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders existing messages from the cache", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    // User message should appear
    expect(screen.getByText("Hello, world!")).toBeInTheDocument();
    // Assistant message should appear
    expect(screen.getByText("Hi there! How can I help?")).toBeInTheDocument();
  });

  it("renders the session title in the header", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    expect(screen.getByText("Test Session")).toBeInTheDocument();
  });

  it("renders the agent badge", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    expect(screen.getByText("test-agent")).toBeInTheDocument();
  });

  it("renders the composer textarea", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    const textarea = screen.getByPlaceholderText("Type a message…");
    expect(textarea).toBeInTheDocument();
  });

  it("renders the send button (disabled when textarea is empty)", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    const sendButton = screen.getByTitle("Send");
    expect(sendButton).toBeDisabled();
  });

  it("enables send button when text is entered", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    const { container } = render(<ChatView sessionId="test-sess" />);

    const textarea = screen.getByPlaceholderText("Type a message…") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "test message" } });

    const sendButton = screen.getByTitle("Send");
    expect(sendButton).not.toBeDisabled();
  });

  it("calls sendMessage when send button is clicked", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    const textarea = screen.getByPlaceholderText("Type a message…") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "hello agent" } });

    const sendButton = screen.getByTitle("Send");
    fireEvent.click(sendButton);

    await waitFor(() => {
      expect(mockSendMessage).toHaveBeenCalledWith("test-sess", "hello agent");
    });
  });

  it("clears the textarea after sending", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    const textarea = screen.getByPlaceholderText("Type a message…") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "clear me" } });

    const sendButton = screen.getByTitle("Send");
    fireEvent.click(sendButton);

    await waitFor(() => {
      expect(textarea.value).toBe("");
    });
  });

  it("renders ASSISTANT label on assistant messages", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    expect(screen.getByText("ASSISTANT")).toBeInTheDocument();
  });

  it("renders copy button on messages", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    const copyButtons = screen.getAllByTitle("Copy");
    expect(copyButtons.length).toBeGreaterThanOrEqual(2); // at least one per message
  });

  it("renders model name from session", async () => {
    const ChatViewModule = await import("./ChatView");
    const ChatView = ChatViewModule.default;
    render(<ChatView sessionId="test-sess" />);

    expect(screen.getByText("test-model")).toBeInTheDocument();
  });
});
