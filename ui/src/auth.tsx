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

  if (loading) return <AuthPanel title="Connecting to Hall…" />;
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

function AuthPanel({ title, children }: { title: string; children?: React.ReactNode }) {
  return <main style={{ minHeight: "100vh", display: "grid", placeItems: "center", background: "var(--bg, #0d0f12)", color: "var(--text, #f4f4f5)" }}>
    <section style={{ width: 340, padding: 28, border: "1px solid #2b3038", borderRadius: 12, background: "#15181d" }}>
      <h1 style={{ margin: "0 0 18px", fontSize: 20 }}>{title}</h1>
      {children}
    </section>
  </main>;
}

function LoginPanel({ error, onLogin }: { error: string; onLogin(username: string, password: string): Promise<void> }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  return <AuthPanel title="Sign in to this Hall">
    <form onSubmit={(event) => {
      event.preventDefault();
      setSubmitting(true);
      void onLogin(username, password).finally(() => setSubmitting(false));
    }} style={{ display: "grid", gap: 12 }}>
      <label>Username<input autoFocus autoComplete="username" required value={username} onChange={(event) => setUsername(event.target.value)} style={{ display: "block", boxSizing: "border-box", width: "100%", marginTop: 6, padding: 10 }} /></label>
      <label>Password<input type="password" autoComplete="current-password" required value={password} onChange={(event) => setPassword(event.target.value)} style={{ display: "block", boxSizing: "border-box", width: "100%", marginTop: 6, padding: 10 }} /></label>
      {error && <p role="alert" style={{ margin: 0, color: "#ff8b8b" }}>{error}</p>}
      <button type="submit" disabled={submitting} style={{ padding: 10 }}>{submitting ? "Signing in…" : "Sign in"}</button>
    </form>
  </AuthPanel>;
}
