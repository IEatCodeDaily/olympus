import { beforeEach, describe, expect, it, vi } from "vitest";
import { getLocalUiState, loadWorkspaceState, saveWorkspaceState } from "./uiState";

vi.mock("../api", () => ({
  apiFetch: vi.fn(() => Promise.reject(new Error("offline"))),
}));

describe("uiState", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  it("writes localStorage synchronously before the remote PUT settles", () => {
    saveWorkspaceState("sessions", { panels: ["session:s-1"] });

    expect(getLocalUiState("sessions")).toEqual({ panels: ["session:s-1"] });
  });

  it("falls back to local state when the remote GET is unavailable", async () => {
    saveWorkspaceState("vault:vault-1", { panels: ["note:one.md"] });

    await expect(loadWorkspaceState("vault:vault-1")).resolves.toEqual({ panels: ["note:one.md"] });
  });
});
