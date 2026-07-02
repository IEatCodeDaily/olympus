import { describe, it, expect } from "vitest";
import { parseRoute } from "./router";

describe("parseRoute", () => {
  it("parses root as sessions list", () => {
    const result = parseRoute("/");
    expect(result.view).toBe("sessions");
    expect(result.sessionId).toBeNull();
  });

  it("parses /sessions/:id with a session id", () => {
    const result = parseRoute("/sessions/abc123");
    expect(result.view).toBe("sessions");
    expect(result.sessionId).toBe("abc123");
  });

  it("parses /sessions/oly- (short id)", () => {
    const result = parseRoute("/sessions/oly-");
    expect(result.view).toBe("sessions");
    expect(result.sessionId).toBe("oly-");
  });

  it("handles empty session id after slash", () => {
    const result = parseRoute("/sessions/");
    expect(result.view).toBe("sessions");
    expect(result.sessionId).toBeNull();
  });

  it("parses /fleet", () => {
    const result = parseRoute("/fleet");
    expect(result.view).toBe("fleet");
    expect(result.sessionId).toBeNull();
  });

  it("parses /agents", () => {
    const result = parseRoute("/agents");
    expect(result.view).toBe("agents");
    expect(result.sessionId).toBeNull();
  });

  it("parses /board", () => {
    const result = parseRoute("/board");
    expect(result.view).toBe("board");
    expect(result.sessionId).toBeNull();
  });

  it("parses /settings", () => {
    const result = parseRoute("/settings");
    expect(result.view).toBe("settings");
    expect(result.sessionId).toBeNull();
  });

  it("falls back to sessions for unknown paths", () => {
    expect(parseRoute("/unknown").view).toBe("sessions");
    expect(parseRoute("").view).toBe("sessions");
  });
});
