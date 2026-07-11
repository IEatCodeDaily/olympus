import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthGate, useHallAuth } from "./auth";

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function CurrentOrganization() {
  const auth = useHallAuth();
  return <div>
    <span>{auth.user.username}:{auth.organization.displayName}</span>
    {auth.organizations.map((organization) => (
      <button key={organization.id} onClick={() => auth.selectOrganization(organization.id)}>
        {organization.displayName}
      </button>
    ))}
  </div>;
}

function renderGate(queryClient = new QueryClient()) {
  render(
    <QueryClientProvider client={queryClient}>
      <AuthGate><CurrentOrganization /></AuthGate>
    </QueryClientProvider>,
  );
  return queryClient;
}

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("AuthGate", () => {
  it("shows Hall-local login and signs in without accepting a Hall URL", async () => {
    let authenticated = false;
    const fetchMock = vi.spyOn(window, "fetch").mockImplementation(async (input, init) => {
      const path = String(input);
      if (path.endsWith("/api/auth/session")) {
        return authenticated
          ? json({ user: { userId: "u1", username: "alice", kind: "user" } })
          : new Response(null, { status: 401 });
      }
      if (path.endsWith("/api/auth/login") && init?.method === "POST") {
        authenticated = true;
        return json({ ok: true });
      }
      if (path.endsWith("/api/organizations")) {
        return json({ organizations: [{ id: "org-a", slug: "a", displayName: "Org A", role: "owner" }] });
      }
      throw new Error(`unexpected request ${path}`);
    });

    renderGate();
    expect(await screen.findByRole("heading", { name: "Sign in to this Hall" })).toBeInTheDocument();
    expect(screen.queryByLabelText(/hall url/i)).not.toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Username"), "alice");
    await userEvent.type(screen.getByLabelText("Password"), "correct horse battery staple");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByText("alice:Org A")).toBeInTheDocument();
    const loginCall = fetchMock.mock.calls.find(([, init]) => init?.method === "POST");
    expect(loginCall?.[1]?.credentials).toBe("include");
    expect(String(loginCall?.[0])).toMatch(/\/api\/auth\/login$/);
  });

  it("shows a live status region while the session resolves", async () => {
    let resolveSession: (r: Response) => void = () => {};
    vi.spyOn(window, "fetch").mockImplementation(async (input) => {
      const path = String(input);
      if (path.endsWith("/api/auth/session")) {
        return new Promise<Response>((resolve) => { resolveSession = resolve; });
      }
      if (path.endsWith("/api/organizations")) {
        return json({ organizations: [{ id: "org-a", slug: "a", displayName: "Org A", role: "owner" }] });
      }
      throw new Error(`unexpected request ${path}`);
    });

    renderGate();
    expect(await screen.findByRole("status")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Connecting to Hall" })).toBeInTheDocument();
    resolveSession(json({ user: { userId: "u1", username: "alice", kind: "user" } }));
    expect(await screen.findByText("alice:Org A")).toBeInTheDocument();
  });

  it("surfaces a bad-credential failure as an alert without leaving the login form", async () => {
    vi.spyOn(window, "fetch").mockImplementation(async (input, init) => {
      const path = String(input);
      if (path.endsWith("/api/auth/session")) return new Response(null, { status: 401 });
      if (path.endsWith("/api/auth/login") && init?.method === "POST") {
        return new Response(null, { status: 401 });
      }
      throw new Error(`unexpected request ${path}`);
    });

    renderGate();
    await userEvent.type(await screen.findByLabelText("Username"), "alice");
    await userEvent.type(screen.getByLabelText("Password"), "wrong");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Invalid username or password.");
    expect(screen.getByRole("heading", { name: "Sign in to this Hall" })).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toHaveAttribute("aria-invalid", "true");
  });

  it("switches only among memberships and clears organization-specific cache", async () => {
    vi.spyOn(window, "fetch").mockImplementation(async (input) => {
      const path = String(input);
      if (path.endsWith("/api/auth/session")) {
        return json({ user: { userId: "u1", username: "alice", kind: "user" } });
      }
      if (path.endsWith("/api/organizations")) {
        return json({ organizations: [
          { id: "org-a", slug: "a", displayName: "Org A", role: "owner" },
          { id: "org-b", slug: "b", displayName: "Org B", role: "member" },
        ] });
      }
      throw new Error(`unexpected request ${path}`);
    });
    const client = new QueryClient();
    const clear = vi.spyOn(client, "clear");
    renderGate(client);

    expect(await screen.findByText("alice:Org A")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Org B" }));

    await waitFor(() => expect(screen.getByText("alice:Org B")).toBeInTheDocument());
    expect(localStorage.getItem("olympus-organization-id")).toBe("org-b");
    expect(clear).toHaveBeenCalledTimes(1);
  });
});
