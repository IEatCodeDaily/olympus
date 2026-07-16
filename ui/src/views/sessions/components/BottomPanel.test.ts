import { describe, expect, it } from "vitest";
import type { Message } from "../../../types";
import { logsFromMessages } from "./BottomPanel";

function message(overrides: Partial<Message>): Message {
  return {
    messageId: 1,
    sessionId: "session-1",
    role: "user",
    content: null,
    toolName: null,
    toolCalls: null,
    reasoning: null,
    timestamp: 10,
    tokenCount: null,
    finishReason: null,
    ...overrides,
  };
}

describe("logsFromMessages", () => {
  it("rehydrates lifecycle logs from Hall-owned messages", () => {
    const logs = logsFromMessages([
      message({ messageId: 1, role: "user", timestamp: 10 }),
      message({
        messageId: 2,
        role: "assistant",
        timestamp: 12,
        toolCalls: [
          {
            id: "tool-1",
            name: "terminal",
            args: { command: "redacted from lifecycle log" },
            status: "completed",
            result: "not copied into lifecycle log",
          },
        ],
        finishReason: "end_turn",
      }),
      message({
        messageId: 3,
        role: "system",
        timestamp: 14,
        content: "⚠ Failed to start agent: adapter failed",
        finishReason: "error",
      }),
    ]);

    expect(logs).toEqual([
      {
        id: "message:1:user",
        ts: 10,
        level: "info",
        source: "olympus",
        message: "User message sent",
      },
      {
        id: "message:2:tool:tool-1",
        ts: 12,
        level: "info",
        source: "agent",
        message: "Tool call: terminal (completed)",
      },
      {
        id: "message:2:done",
        ts: 12,
        level: "info",
        source: "agent",
        message: "Turn finished: end_turn",
      },
      {
        id: "message:3:system",
        ts: 14,
        level: "error",
        source: "olympus",
        message: "⚠ Failed to start agent: adapter failed",
      },
    ]);
  });

  it("does not copy prompts, tool arguments, results, or reasoning into logs", () => {
    const serialized = JSON.stringify(
      logsFromMessages([
        message({ messageId: 1, content: "SECRET PROMPT" }),
        message({
          messageId: 2,
          role: "assistant",
          reasoning: "SECRET REASONING",
          toolCalls: [
            {
              name: "read_file",
              args: { path: "SECRET PATH" },
              result: "SECRET RESULT",
            },
          ],
        }),
      ]),
    );

    expect(serialized).not.toContain("SECRET PROMPT");
    expect(serialized).not.toContain("SECRET REASONING");
    expect(serialized).not.toContain("SECRET PATH");
    expect(serialized).not.toContain("SECRET RESULT");
  });
});
