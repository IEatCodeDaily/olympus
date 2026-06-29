import { useEffect, useMemo, useState } from "react";
import { Badge, EmptyState, PageHeader, StatPill } from "../components/shell";
import { healthCheck } from "../api";
import { THEMES, THEME_LABELS, useTheme } from "../lib/theme";
import type { HealthResponse } from "../types";

type Density = "comfortable" | "compact";

const DENSITY_STORAGE_KEY = "olympus-density";
const DENSITY_OPTIONS: Array<{ value: Density; label: string; blurb: string }> = [
  { value: "comfortable", label: "Comfortable", blurb: "Breathing room for longer operator sessions." },
  { value: "compact", label: "Compact", blurb: "Tighter rows and chrome for high-volume scanning." },
];

const API_TOKEN = (import.meta.env.VITE_API_TOKEN as string | undefined) ?? "";
const CODER_MODEL = (import.meta.env.VITE_CODER_MODEL as string | undefined) ?? "";
const REVIEWER_MODEL = (import.meta.env.VITE_REVIEWER_MODEL as string | undefined) ?? "";

function getStoredDensity(): Density {
  if (typeof localStorage === "undefined") return "comfortable";
  const density = localStorage.getItem(DENSITY_STORAGE_KEY);
  return density === "compact" ? "compact" : "comfortable";
}

function applyDensity(density: Density): void {
  document.documentElement.dataset.density = density;
}

function maskToken(token: string): string {
  if (!token) return "No VITE_API_TOKEN configured";
  if (token.length <= 8) return "•".repeat(token.length);
  return `${token.slice(0, 4)}${"•".repeat(Math.max(4, token.length - 8))}${token.slice(-4)}`;
}

function formatThemeSummary(theme: (typeof THEMES)[number]): string {
  if (theme === "midnight") return "Dark default";
  if (theme === "daylight") return "Light contrast";
  return "Warm terminal";
}

export default function SettingsView() {
  const [theme, setTheme] = useTheme();
  const [density, setDensityState] = useState<Density>(getStoredDensity);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [healthState, setHealthState] = useState<"loading" | "ready" | "error">("loading");
  const [tokenRevealed, setTokenRevealed] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");

  useEffect(() => {
    applyDensity(density);
    try {
      localStorage.setItem(DENSITY_STORAGE_KEY, density);
    } catch {
      // ignore storage failures
    }
  }, [density]);

  useEffect(() => {
    let cancelled = false;
    setHealthState("loading");

    healthCheck()
      .then((data) => {
        if (cancelled) return;
        setHealth(data);
        setHealthState("ready");
      })
      .catch(() => {
        if (cancelled) return;
        setHealthState("error");
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const routingRows = useMemo(
    () => [
      {
        label: "Coder route",
        model: CODER_MODEL,
        note: CODER_MODEL
          ? "Explicit UI override from build-time env."
          : "Not exposed yet — Olympus only returns health today, so this stays informational.",
      },
      {
        label: "Reviewer route",
        model: REVIEWER_MODEL,
        note: REVIEWER_MODEL
          ? "Explicit UI override from build-time env."
          : "Promote /api/health or /api/models when reviewer routing is surfaced by the backend.",
      },
    ],
    []
  );

  async function handleCopyToken(): Promise<void> {
    if (!API_TOKEN) return;
    try {
      await navigator.clipboard.writeText(API_TOKEN);
      setCopyState("copied");
    } catch {
      try {
        const textarea = document.createElement("textarea");
        textarea.value = API_TOKEN;
        textarea.setAttribute("readonly", "true");
        textarea.style.position = "absolute";
        textarea.style.left = "-9999px";
        document.body.appendChild(textarea);
        textarea.select();
        textarea.setSelectionRange(0, textarea.value.length);
        const copied = document.execCommand("copy");
        document.body.removeChild(textarea);
        setCopyState(copied ? "copied" : "error");
      } catch {
        setCopyState("error");
      }
    }
  }

  return (
    <div className="view-scroll">
      <PageHeader title="Settings" subtitle="Appearance, runtime posture, and read-only operator preferences" />

      <div className="board-stats">
        <StatPill label="theme" value={THEME_LABELS[theme]} />
        <StatPill label="density" value={density} />
        <StatPill label="health" value={healthState === "ready" ? "live" : healthState} />
      </div>

      <section className="settings-section settings-section-wide">
        <h2 className="settings-heading">Appearance</h2>
        <p className="settings-hint">Theme is live. Density rewrites layout spacing across the shell so the cockpit can trade comfort for scan speed.</p>

        <div className="settings-panel">
          <div className="settings-panel-block">
            <div className="settings-section-label-row">
              <span className="settings-section-label">Theme palette</span>
              <Badge>{formatThemeSummary(theme)}</Badge>
            </div>
            <div className="theme-grid">
              {THEMES.map((t) => (
                <button
                  key={t}
                  type="button"
                  className={`theme-swatch ${theme === t ? "active" : ""}`}
                  data-theme={t}
                  aria-pressed={theme === t}
                  onClick={() => setTheme(t)}
                >
                  <span className="theme-swatch-preview">
                    <span className="sw sw-bg" />
                    <span className="sw sw-accent" />
                    <span className="sw sw-text" />
                  </span>
                  <span className="theme-swatch-meta">
                    <span className="theme-swatch-label">{THEME_LABELS[t]}</span>
                    <span className="theme-swatch-copy">{formatThemeSummary(t)}</span>
                  </span>
                </button>
              ))}
            </div>
          </div>

          <div className="settings-panel-block">
            <div className="settings-section-label-row">
              <span className="settings-section-label">Density</span>
              <Badge>{density}</Badge>
            </div>

            <div className="settings-density-grid">
              {DENSITY_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={`settings-density-btn ${density === option.value ? "active" : ""}`}
                  aria-pressed={density === option.value}
                  onClick={() => setDensityState(option.value)}
                >
                  <span className="settings-density-title">{option.label}</span>
                  <span className="settings-density-copy">{option.blurb}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="settings-section settings-section-wide">
        <h2 className="settings-heading">Access token</h2>
        <p className="settings-hint">The UI reads the API token at build time. Keep it read-only here so operators can inspect or copy without mutating deployment state.</p>

        <div className="settings-token-card">
          <div className="settings-token-main">
            <div className="settings-section-label-row">
              <span className="settings-section-label">VITE_API_TOKEN</span>
              <Badge>{API_TOKEN ? "configured" : "missing"}</Badge>
            </div>
            <code className="settings-token-value">{tokenRevealed ? API_TOKEN || "No token configured" : maskToken(API_TOKEN)}</code>
            <p className="settings-token-note">Masked by default. Copy uses the full token when present.</p>
          </div>
          <div className="settings-token-actions">
            <button
              type="button"
              className="settings-action-btn"
              onClick={() => setTokenRevealed((current) => !current)}
              disabled={!API_TOKEN}
            >
              {tokenRevealed ? "Hide" : "Reveal"}
            </button>
            <button
              type="button"
              className="settings-action-btn"
              onClick={() => void handleCopyToken()}
              disabled={!API_TOKEN}
            >
              {copyState === "copied" ? "Copied" : copyState === "error" ? "Copy failed" : "Copy"}
            </button>
          </div>
        </div>
      </section>

      <section className="settings-section settings-section-wide">
        <h2 className="settings-heading">Model routing</h2>
        <p className="settings-hint">Show the operator where coding and review work will land. Until the backend exposes role-specific routing, keep the display explicit about that gap.</p>

        <div className="settings-routing-grid">
          {routingRows.map((row) => (
            <div key={row.label} className="settings-routing-card">
              <div className="settings-section-label-row">
                <span className="settings-section-label">{row.label}</span>
                <Badge>{row.model ? "explicit" : "pending"}</Badge>
              </div>
              <div className="settings-routing-model">{row.model || "Backend field not available yet"}</div>
              <p className="settings-routing-note">{row.note}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="settings-section settings-section-wide">
        <h2 className="settings-heading">Runtime</h2>
        <p className="settings-hint">Live control-plane health and a quick read on the imported Hermes snapshot.</p>

        {healthState === "loading" ? (
          <div className="settings-runtime-skeleton">
            <div className="skel-line" />
            <div className="skel-line" />
            <div className="skel-line" />
            <div className="skel-line" />
          </div>
        ) : healthState === "error" || !health ? (
          <EmptyState
            title="Health endpoint unavailable"
            message="Settings can still manage local appearance, but runtime details stay read-only until /api/health responds again."
          />
        ) : (
          <div className="settings-rows">
            <div className="settings-row">
              <span>Hermes profile</span>
              <span>{health.hermesProfile}</span>
            </div>
            <div className="settings-row">
              <span>Import state</span>
              <span>{health.importState}</span>
            </div>
            <div className="settings-row">
              <span>Sync link</span>
              <span>{health.syncConnected ? "connected" : "disconnected"}</span>
            </div>
            <div className="settings-row">
              <span>Snapshot</span>
              <span>
                {health.snapshot ? `${health.snapshot.sessions} sessions · ${health.snapshot.messages} messages` : "No snapshot loaded"}
              </span>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
