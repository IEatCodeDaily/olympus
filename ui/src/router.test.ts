import { describe, it, expect } from "vitest";
import { parseRoute } from "./router";

describe("parseRoute", () => {
  it("parses root as sessions list", () => {
    const result = parseRoute("/");
    expect(result.surface).toBe("sessions");
    expect(result.sessionId).toBeNull();
  });

  it("parses /sessions/:id with a session id", () => {
    const result = parseRoute("/sessions/abc123");
    expect(result.surface).toBe("sessions");
    expect(result.sessionId).toBe("abc123");
  });

  it("parses /sessions/oly- (short id)", () => {
    const result = parseRoute("/sessions/oly-");
    expect(result.surface).toBe("sessions");
    expect(result.sessionId).toBe("oly-");
  });

  it("handles empty session id after slash", () => {
    const result = parseRoute("/sessions/");
    expect(result.surface).toBe("sessions");
    expect(result.sessionId).toBeNull();
  });

  it("parses /vaults", () => {
    const result = parseRoute("/vaults");
    expect(result.surface).toBe("vaults");
    expect(result.sessionId).toBeNull();
  });

  it("parses /projects", () => {
    const result = parseRoute("/projects");
    expect(result.surface).toBe("projects");
    expect(result.sessionId).toBeNull();
  });

  it("parses /fleet", () => {
    const result = parseRoute("/fleet");
    expect(result.surface).toBe("fleet");
    expect(result.sessionId).toBeNull();
  });

  it("parses /settings", () => {
    const result = parseRoute("/settings");
    expect(result.surface).toBe("settings");
    expect(result.sessionId).toBeNull();
  });

  it("falls back to sessions for unknown paths", () => {
    expect(parseRoute("/unknown").surface).toBe("sessions");
    expect(parseRoute("").surface).toBe("sessions");
  });
});
