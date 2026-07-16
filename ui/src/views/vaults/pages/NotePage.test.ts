import { describe, expect, it } from "vitest";
import { shouldClearDirtyAfterSave } from "./NotePage";

describe("Vault note save snapshot", () => {
  it("clears dirty only when the live draft still equals the submitted snapshot", () => {
    expect(shouldClearDirtyAfterSave("submitted", "submitted")).toBe(true);
    expect(shouldClearDirtyAfterSave("typed while saving", "submitted")).toBe(false);
  });
});
