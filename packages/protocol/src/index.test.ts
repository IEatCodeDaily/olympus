import { describe, expect, it } from "bun:test";
import {
  isRuntimeCommand,
  parseRuntimeCommand,
  RUNTIME_COMMAND_KINDS,
  type RuntimeCommand,
} from "./index";

describe("runtime protocol", () => {
  it("accepts a valid agent.run.start command", () => {
    const cmd: RuntimeCommand = {
      kind: "agent.run.start",
      commandId: "c1",
      profileId: "default",
      sessionId: "s1",
      input: "PONG",
    };
    expect(isRuntimeCommand(cmd)).toBe(true);
    expect(parseRuntimeCommand(cmd)).toEqual(cmd);
  });

  it("rejects payloads without a known kind", () => {
    expect(isRuntimeCommand({ kind: "nope", commandId: "x" })).toBe(false);
    expect(() => parseRuntimeCommand({ commandId: "x" })).toThrow();
  });

  it("rejects payloads without a commandId", () => {
    expect(isRuntimeCommand({ kind: "fs.read" })).toBe(false);
  });

  it("exposes all command kinds", () => {
    expect(RUNTIME_COMMAND_KINDS).toContain("agent.run.start");
    expect(RUNTIME_COMMAND_KINDS).toContain("terminal.open");
    expect(RUNTIME_COMMAND_KINDS.length).toBe(5);
  });
});
