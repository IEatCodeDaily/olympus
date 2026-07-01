// Theme system — theme-addressable UI (see docs/plans/2026-06-29-olympus-ui-roadmap.md).
// Themes are defined as CSS-variable blocks under [data-theme] in index.css.
// This module owns the runtime switch + persistence.
import { useState, useEffect, useCallback } from "react";

export const THEMES = ["midnight", "daylight", "amber-crt"] as const;
export type Theme = (typeof THEMES)[number];

export const THEME_LABELS: Record<Theme, string> = {
  midnight: "Midnight",
  daylight: "Daylight",
  "amber-crt": "Amber CRT",
};

const STORAGE_KEY = "olympus-theme";

function getStoredTheme(): Theme {
  const t = (typeof localStorage !== "undefined" && localStorage.getItem(STORAGE_KEY)) as Theme | null;
  return t && (THEMES as readonly string[]).includes(t) ? t : "midnight";
}

function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

/** Hook: current theme + setter that persists and applies to <html>. */
export function useTheme(): [Theme, (t: Theme) => void] {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const setTheme = useCallback((t: Theme) => {
    try {
      localStorage.setItem(STORAGE_KEY, t);
    } catch {
      // ignore storage failures (private mode etc.)
    }
    setThemeState(t);
  }, []);

  return [theme, setTheme];
}
