import { beforeEach, describe, expect, it } from "vitest";
import { readSessionPanelState, writeSessionPanelState } from "./sessionPanelState";

describe("session panel state", () => {
  beforeEach(() => localStorage.clear());

  it("isolates panel state by session window", () => {
    writeSessionPanelState("one", "rsCollapsed", true);
    writeSessionPanelState("two", "rsCollapsed", false);

    expect(readSessionPanelState("one", "rsCollapsed", false)).toBe(true);
    expect(readSessionPanelState("two", "rsCollapsed", true)).toBe(false);
  });
});
