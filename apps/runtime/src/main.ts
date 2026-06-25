/**
 * Olympus Bun host runtime adapter.
 *
 * Responsibilities (milestone 1):
 *  - connect to Convex
 *  - publish heartbeat every HEARTBEAT_MS
 *  - poll/claim pending runtime commands and (later) execute via Hermes adapter
 *  - expose a localhost /healthz endpoint
 *
 * Host effects (process spawn, PTY, fs) land in later tasks. This file keeps the
 * runtime intentionally small — Convex is the brain, this is the hands.
 */

import { ConvexClient } from "convex/browser";
import { anyApi } from "convex/server";
import { HermesAgentRuntime } from "@olympus/hermes-adapter";

const CONVEX_URL = process.env.CONVEX_URL ?? "";
const RUNTIME_ID = process.env.OLYMPUS_RUNTIME_ID ?? "local-dev";
const HEALTH_PORT = Number(process.env.OLYMPUS_RUNTIME_HEALTH_PORT ?? "8791");
const HEARTBEAT_MS = 10_000;
const CLAIM_POLL_MS = 2_000;

const api = anyApi;
const hermes = new HermesAgentRuntime({ profile: process.env.OLYMPUS_PROFILE ?? "default" });

let lastHeartbeatAt = 0;
let lastError: string | null = null;

function log(msg: string, extra?: unknown) {
  const line = `[olympus-runtime] ${new Date().toISOString()} ${msg}`;
  if (extra !== undefined) console.log(line, extra);
  else console.log(line);
}

async function startHealthServer() {
  Bun.serve({
    port: HEALTH_PORT,
    hostname: "127.0.0.1",
    fetch(req) {
      const url = new URL(req.url);
      if (url.pathname === "/healthz") {
        return Response.json({
          ok: true,
          runtimeId: RUNTIME_ID,
          convexConfigured: CONVEX_URL.length > 0,
          lastHeartbeatAt,
          hermes: hermes.describe(),
          lastError,
        });
      }
      return new Response("not found", { status: 404 });
    },
  });
  log(`health endpoint on http://127.0.0.1:${HEALTH_PORT}/healthz`);
}

async function heartbeatLoop(client: ConvexClient) {
  for (;;) {
    try {
      await client.mutation(api.runtime.heartbeat, { runtimeId: RUNTIME_ID });
      lastHeartbeatAt = Date.now();
      lastError = null;
    } catch (err) {
      lastError = err instanceof Error ? err.message : String(err);
      log("heartbeat failed", lastError);
    }
    await Bun.sleep(HEARTBEAT_MS);
  }
}

async function claimLoop(client: ConvexClient) {
  for (;;) {
    try {
      const cmd = await client.mutation(api.commands.claimNext, { runtimeId: RUNTIME_ID });
      if (cmd) {
        log(`claimed command ${cmd._id} kind=${cmd.kind}`);
        // TODO(milestone 2): dispatch via Hermes adapter, stream events back.
        await client.mutation(api.commands.complete, { commandId: cmd._id, status: "done" });
      }
    } catch (err) {
      lastError = err instanceof Error ? err.message : String(err);
      log("claim loop error", lastError);
    }
    await Bun.sleep(CLAIM_POLL_MS);
  }
}

async function main() {
  await startHealthServer();
  if (!CONVEX_URL) {
    log("CONVEX_URL not set — running in health-only mode (set CONVEX_URL to connect).");
    return;
  }
  const client = new ConvexClient(CONVEX_URL);
  log(`connecting to Convex at ${CONVEX_URL} as runtimeId=${RUNTIME_ID}`);
  void heartbeatLoop(client);
  void claimLoop(client);
}

main().catch((err) => {
  log("fatal", err instanceof Error ? err.message : String(err));
  process.exit(1);
});
