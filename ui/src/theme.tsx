// ThemeProvider — manages the Olympus color theme (obsidian / light).
//
// Persists to localStorage["olympus-theme"], applies via
// document.documentElement.dataset.theme. Default is "obsidian" (dark).
//
// Density is also managed here (comfortable / compact) via
// document.documentElement.dataset.density, persisted separately.

import {
  createContext,
  useContext,
  useEffect,
  useCallback,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type Theme = "obsidian" | "light";
export type Density = "comfortable" | "compact";

const THEME_KEY = "olympus-theme";
const DENSITY_KEY = "olympus-density";

interface ThemeContextValue {
  theme: Theme;
  density: Density;
  setTheme: (t: Theme) => void;
  toggleTheme: () => void;
  setDensity: (d: Density) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readStored(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // ignore quota / privacy errors
  }
}

function isValidTheme(v: string): v is Theme {
  return v === "obsidian" || v === "light";
}

function isValidDensity(v: string): v is Density {
  return v === "comfortable" || v === "compact";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  // Read once on mount — SSR-safe (no window access during initial render).
  const [theme, setThemeState] = useState<Theme>(() => {
    if (typeof window === "undefined") return "obsidian";
    const stored = readStored(THEME_KEY, "obsidian");
    return isValidTheme(stored) ? stored : "obsidian";
  });
  const [density, setDensityState] = useState<Density>(() => {
    if (typeof window === "undefined") return "comfortable";
    const stored = readStored(DENSITY_KEY, "comfortable");
    return isValidDensity(stored) ? stored : "comfortable";
  });

  // Apply theme to <html data-theme="...">
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Apply density to <html data-density="...">
  useEffect(() => {
    document.documentElement.dataset.density = density;
  }, [density]);

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    writeStored(THEME_KEY, t);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((prev) => {
      const next: Theme = prev === "obsidian" ? "light" : "obsidian";
      writeStored(THEME_KEY, next);
      return next;
    });
  }, []);

  const setDensity = useCallback((d: Density) => {
    setDensityState(d);
    writeStored(DENSITY_KEY, d);
  }, []);

  const value = useMemo(
    () => ({ theme, density, setTheme, toggleTheme, setDensity }),
    [theme, density, setTheme, toggleTheme, setDensity],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
