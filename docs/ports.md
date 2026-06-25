# Olympus — Port & Workspace Allocation (source of truth)

Each Hermes profile gets its own git clone of the olympus repo and a reserved,
non-overlapping port range so agent runtimes/Convex/web/e2e never collide with
the protected owner deployment or with each other.

NO separate Linux users: the Docker daemon is shared (rpw is in the `docker`
group) and Hermes profiles + the kanban dispatcher all run as `rpw`, so separate
users add friction without real isolation. Port-range reservation + per-profile
workspace clones provide the isolation instead.

## Allocation table

| Owner | Workspace | Convex cloud | Convex site | Convex dashboard | runtime /healthz | web (Vite) | e2e range |
|---|---|---:|---:|---:|---:|---:|---:|
| **owner deployment (PROTECTED)** | `/home/rpw/olympus` | 3210 | 3211 | 6791 | 8791 | 5177 | — |
| coding-agent | `~/.hermes/profiles/coding-agent/workspace/olympus` | 3220 | 3221 | 6792 | 8792 | 5178 | — |
| code-reviewer | `~/.hermes/profiles/code-reviewer/workspace/olympus` | 3230 | 3231 | 6793 | 8793 | 5179 | — |
| tester | `~/.hermes/profiles/tester/workspace/olympus` | 3240 (ephemeral) | 3241 | 6794 | 8794 | 5180 | 5190–5199 |

## Rules

- The owner deployment ports (3210/3211/6791/8791/5177) are RESERVED. No agent
  profile may bind them. Agents work only inside their own workspace clone.
- tester's Convex is EPHEMERAL: each e2e run starts a throwaway `convex dev`
  local backend in its workspace (own data dir, port 3240) and tears it down
  after. No persistent tester Convex container.
- coding-agent / code-reviewer rarely need a live backend; when they do, they use
  their reserved ports, never the owner's.
- Studio (hermes-web-ui) owns :8787 and must never be touched by Olympus work.

## Owner systemd services (durable)

- `olympus-convex.service` — docker compose (3210/3211/6791)
- `olympus-runtime.service` — Bun runtime (8791)
- `olympus-web.service` — Vite web (5177)
