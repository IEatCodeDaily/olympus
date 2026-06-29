// SettingsView — theme switcher (functional) + profile/routing/token (read-only
// placeholders for now). Roadmap U2. The theme switcher is fully wired.
import { useEffect, useState } from "react";
import { PageHeader } from "../components/shell";
import { THEMES, THEME_LABELS, useTheme } from "../lib/theme";
import { healthCheck } from "../api";
import type { HealthResponse } from "../types";

export default function SettingsView() {
  const [theme, setTheme] = useTheme();
  const [health, setHealth] = useState<HealthResponse | null>(null);

  useEffect(() => {
    healthCheck().then(setHealth).catch(() => {});
  }, []);

  return (
    <div className="view-scroll">
      <PageHeader title="Settings" subtitle="Appearance, profile, and routing" />

      <section className="settings-section">
        <h2 className="settings-heading">Theme</h2>
        <p className="settings-hint">The whole UI is theme-addressable. Pick a palette.</p>
        <div className="theme-grid">
          {THEMES.map((t) => (
            <button
              key={t}
              className={`theme-swatch ${theme === t ? "active" : ""}`}
              data-theme={t}
              onClick={() => setTheme(t)}
            >
              <span className="theme-swatch-preview">
                <span className="sw sw-bg" />
                <span className="sw sw-accent" />
                <span className="sw sw-text" />
              </span>
              <span className="theme-swatch-label">{THEME_LABELS[t]}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-heading">Runtime</h2>
        <div className="settings-rows">
          <div className="settings-row"><span>Hermes profile</span><span>{health?.hermesProfile ?? "—"}</span></div>
          <div className="settings-row"><span>Import state</span><span>{health?.importState ?? "—"}</span></div>
          <div className="settings-row"><span>Sessions</span><span>{health?.snapshot?.sessions ?? "—"}</span></div>
          <div className="settings-row"><span>Messages</span><span>{health?.snapshot?.messages ?? "—"}</span></div>
        </div>
      </section>
    </div>
  );
}
