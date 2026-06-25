import { useQuery } from "convex/react";
import { anyApi } from "convex/server";

const api = anyApi;

export function App() {
  // Subscribes to runtime heartbeat status; undefined while loading.
  const runtimes = useQuery(api.runtime.status) as
    | Array<{ runtimeId: string; at: number; online: boolean }>
    | undefined;

  const anyOnline = Array.isArray(runtimes) && runtimes.some((r) => r.online);

  return (
    <main style={{ fontFamily: "system-ui, sans-serif", padding: 32, maxWidth: 640 }}>
      <h1 style={{ marginBottom: 4 }}>Olympus</h1>
      <p style={{ color: "#666", marginTop: 0 }}>
        AI control plane for Hermes — React + Convex + Bun
      </p>

      <section style={{ marginTop: 24 }}>
        <h2 style={{ fontSize: 16 }}>System</h2>
        <ul style={{ lineHeight: 1.8 }}>
          <li>
            Convex:{" "}
            <strong style={{ color: runtimes === undefined ? "#b45309" : "#15803d" }}>
              {runtimes === undefined ? "connecting…" : "connected"}
            </strong>
          </li>
          <li>
            Olympus Runtime:{" "}
            <strong style={{ color: anyOnline ? "#15803d" : "#b91c1c" }}>
              {anyOnline ? "online" : "offline"}
            </strong>
          </li>
        </ul>
      </section>

      {Array.isArray(runtimes) && runtimes.length > 0 && (
        <section style={{ marginTop: 16 }}>
          <h2 style={{ fontSize: 16 }}>Runtime instances</h2>
          <ul>
            {runtimes.map((r) => (
              <li key={r.runtimeId}>
                {r.runtimeId} — {r.online ? "online" : "offline"} (last seen{" "}
                {new Date(r.at).toLocaleTimeString()})
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}
