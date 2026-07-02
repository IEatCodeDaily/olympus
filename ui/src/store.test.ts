import { describe, it, expect, beforeEach } from "vitest";
import { useUIStore } from "./store";

describe("useUIStore", () => {
  beforeEach(() => {
    // Reset to defaults before each test
    useUIStore.setState({
      view: "sessions",
      activeSessionId: null,
      sidebarCollapsed: false,
      bottomCollapsed: true,
      rightSidebarCollapsed: false,
      bottomTab: "events",
      rightTab: "info",
      paletteOpen: false,
      sidebarWidth: 220,
    });
  });

  it("starts with default values", () => {
    const state = useUIStore.getState();
    expect(state.view).toBe("sessions");
    expect(state.activeSessionId).toBeNull();
    expect(state.sidebarCollapsed).toBe(false);
    expect(state.bottomCollapsed).toBe(true);
  });

  it("toggles sidebar collapse", () => {
    expect(useUIStore.getState().sidebarCollapsed).toBe(false);
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarCollapsed).toBe(true);
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarCollapsed).toBe(false);
  });

  it("toggles bottom panel", () => {
    expect(useUIStore.getState().bottomCollapsed).toBe(true);
    useUIStore.getState().toggleBottom();
    expect(useUIStore.getState().bottomCollapsed).toBe(false);
  });

  it("sets active session", () => {
    useUIStore.getState().setActiveSession("sess-123");
    expect(useUIStore.getState().activeSessionId).toBe("sess-123");
  });

  it("clamps sidebar width to [160, 380]", () => {
    useUIStore.getState().setSidebarWidth(100);
    expect(useUIStore.getState().sidebarWidth).toBe(160);
    useUIStore.getState().setSidebarWidth(500);
    expect(useUIStore.getState().sidebarWidth).toBe(380);
    useUIStore.getState().setSidebarWidth(250);
    expect(useUIStore.getState().sidebarWidth).toBe(250);
  });

  it("sets bottom tab", () => {
    useUIStore.getState().setBottomTab("logs");
    expect(useUIStore.getState().bottomTab).toBe("logs");
  });

  it("sets right sidebar tab", () => {
    useUIStore.getState().setRightTab("artifacts");
    expect(useUIStore.getState().rightTab).toBe("artifacts");
  });

  it("opens and closes command palette", () => {
    useUIStore.getState().setPaletteOpen(true);
    expect(useUIStore.getState().paletteOpen).toBe(true);
    useUIStore.getState().setPaletteOpen(false);
    expect(useUIStore.getState().paletteOpen).toBe(false);
  });
});
