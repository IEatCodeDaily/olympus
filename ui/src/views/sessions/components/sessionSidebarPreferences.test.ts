import { describe, expect, it } from "vitest";
import type { Session } from "../../../types";
import { sessionMetadata, toggleSessionMetadataField } from "./sessionSidebarPreferences";

const session = {
  agent: "codex",
  model: "gpt-5.4",
  node: "terminus",
  source: "acp",
  messageCount: 42,
  inputTokens: 1200,
  outputTokens: 800,
} as Session;

describe("session sidebar metadata preferences", () => {
  it("renders only selected metadata in stable field order", () => {
    expect(sessionMetadata(session, new Set(["model", "agent", "tokens"]))).toEqual([
      "codex",
      "gpt-5.4",
      "2k tok",
    ]);
  });

  it("toggles fields without mutating the existing selection", () => {
    const current = new Set(["agent"] as const);
    const added = toggleSessionMetadataField(current, "node");
    expect([...current]).toEqual(["agent"]);
    expect([...added]).toEqual(["agent", "node"]);
    expect([...toggleSessionMetadataField(added, "agent")]).toEqual(["node"]);
  });
});
