import React, { createContext, useContext, useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { closeWs, setApiOrganization } from "./api";

// Production identity requests are permanently bound to the Hall origin that
// served the UI. A separate API base exists only for Vite development.
const BASE = import.meta.env.DEV ? (import.meta.env.VITE_API_BASE as string) : "";

export interface HallUser {
  userId: string;
  username: string;
  kind: "user";
}

export interface HallOrganization {
  id: string;
  slug: string;
  displayName: string;
  role: string;
}

interface AuthContextValue {
  user: HallUser;
  organizations: HallOrganization[];
  organization: HallOrganization;
  selectOrganization(id: string): void;
  logout(): Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

async function hallFetch(path: string, init?: RequestInit): Promise<Response> {
  return window.fetch(`${BASE}${path}`, { ...init, credentials: "include" });
}

export function useHallAuth(): AuthContextValue {
  const value = useContext(AuthContext);
  if (!value) throw new Error("useHallAuth must be used inside AuthGate");
  return value;
}

export function AuthGate({ children }: { children: React.ReactNode }) {
  const queryClient = useQueryClient();
  const [user, setUser] = useState<HallUser | null>(null);
  const [organizations, setOrganizations] = useState<HallOrganization[]>([]);
  const [organizationId, setOrganizationId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  async function loadIdentity(): Promise<boolean> {
    const session = await hallFetch("/api/auth/session");
    if (session.status === 401) return false;
    if (!session.ok) throw new Error(`session ${session.status}`);
    const sessionBody = await session.json() as { user: HallUser };
    const memberships = await hallFetch("/api/organizations");
    if (!memberships.ok) throw new Error(`organizations ${memberships.status}`);
    const membershipBody = await memberships.json() as { organizations: HallOrganization[] };
    if (membershipBody.organizations.length === 0) {
      throw new Error("Your account does not belong to an organization.");
    }
    const stored = localStorage.getItem("olympus-organization-id");
    const selected = membershipBody.organizations.find((org) => org.id === stored)
      ?? membershipBody.organizations[0];
    setUser(sessionBody.user);
    setOrganizations(membershipBody.organizations);
    setOrganizationId(selected.id);
    setApiOrganization(selected.id);
    return true;
  }

  useEffect(() => {
    void loadIdentity()
      .catch((cause: unknown) => setError(cause instanceof Error ? cause.message : "Authentication unavailable"))
      .finally(() => setLoading(false));
  }, []);

  function selectOrganization(id: string): void {
    if (!organizations.some((org) => org.id === id)) return;
    localStorage.setItem("olympus-organization-id", id);
    setOrganizationId(id);
    setApiOrganization(id);
    queryClient.clear();
  }

  async function logout(): Promise<void> {
    await hallFetch("/api/auth/logout", { method: "POST" });
    closeWs();
    setApiOrganization(null);
    queryClient.clear();
    setUser(null);
    setOrganizations([]);
    setOrganizationId(null);
  }

  const organization = organizations.find((org) => org.id === organizationId) ?? null;
  const value = useMemo(() => user && organization ? {
    user,
    organizations,
    organization,
    selectOrganization,
    logout,
  } : null, [user, organizations, organization]);

  if (loading) return <LoadingPanel />;
  if (!value) return <LoginPanel error={error} onLogin={async (username, password) => {
    setError("");
    const response = await hallFetch("/api/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!response.ok) {
      setError(response.status === 401 ? "Invalid username or password." : `Login failed (${response.status}).`);
      return;
    }
    try {
      await loadIdentity();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Authentication unavailable");
    }
  }} />;

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

// The Hall origin identity requests are bound to. Shown (read-only) so the
// operator can see which Hall they are signing into — never an editable field.
const HALL_HOST = typeof window !== "undefined" ? window.location.host : "";

// Restrained monochrome Olympus mark: twin ascending peaks (altitude / signal),
// stroked in the accent. Purely decorative — hidden from assistive tech.
function AuthMark() {
  return <svg className="auth-brandmark" width="24" height="24" viewBox="0 0 24 24" fill="none"
    stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M4 13 L12 6 L20 13" />
    <path d="M4 18 L12 11 L20 18" />
  </svg>;
}

function AuthShell({ title, subtitle, busy, children }: {
  title: string; subtitle: string; busy?: boolean; children: React.ReactNode;
}) {
  return <main className="auth-screen">
    <section className="auth-card" aria-busy={busy || undefined}>
      <header className="auth-head">
        <span className="auth-kicker">Control plane</span>
        <div className="auth-brand"><AuthMark /><span className="auth-wordmark">Olympus</span></div>
        <h1 className="auth-title">{title}</h1>
        <p className="auth-sub">{subtitle}</p>
      </header>
      {children}
      <footer className="auth-foot">
        <span className="auth-foot-dot" aria-hidden="true" />
        <span className="auth-foot-key">Hall</span>
        {HALL_HOST && <span className="auth-foot-host">{HALL_HOST}</span>}
      </footer>
    </section>
  </main>;
}

function LoadingPanel() {
  return <AuthShell title="Connecting to Hall" subtitle="Establishing a secure session." busy>
    <div className="auth-status" role="status">
      <span className="ol-spinner ol-spinner-lg" aria-hidden="true" />
      <span className="auth-status-text">Connecting…</span>
    </div>
  </AuthShell>;
}

function LoginPanel({ error, onLogin }: { error: string; onLogin(username: string, password: string): Promise<void> }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const errorId = "auth-error";
  return <AuthShell title="Sign in to this Hall" subtitle="Enter your operator credentials to continue.">
    <form className="auth-form" onSubmit={(event) => {
      event.preventDefault();
      setSubmitting(true);
      void onLogin(username, password).finally(() => setSubmitting(false));
    }}>
      <label className="auth-field">
        <span className="ol-field-label">Username</span>
        <input className="ol-input" autoFocus autoComplete="username" required
          aria-invalid={error ? true : undefined} aria-describedby={error ? errorId : undefined}
          value={username} onChange={(event) => setUsername(event.target.value)} />
      </label>
      <label className="auth-field">
        <span className="ol-field-label">Password</span>
        <input className="ol-input" type="password" autoComplete="current-password" required
          aria-invalid={error ? true : undefined} aria-describedby={error ? errorId : undefined}
          value={password} onChange={(event) => setPassword(event.target.value)} />
      </label>
      {error && <p className="auth-error" role="alert" id={errorId}>{error}</p>}
      <button type="submit" className="ol-btn ol-btn-primary ol-btn-block auth-submit" disabled={submitting}>
        {submitting ? "Signing in…" : "Sign in"}
      </button>
    </form>
  </AuthShell>;
}
