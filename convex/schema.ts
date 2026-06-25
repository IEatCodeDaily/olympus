import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

/**
 * Olympus control-plane schema. Convex owns durable truth and orchestration
 * intent. The Bun runtime claims commands and writes events; React subscribes.
 */
export default defineSchema({
  profiles: defineTable({
    name: v.string(),
    description: v.optional(v.string()),
  }).index("by_name", ["name"]),

  sessions: defineTable({
    profileId: v.string(),
    title: v.optional(v.string()),
    createdAt: v.number(),
  }).index("by_profile", ["profileId"]),

  messages: defineTable({
    sessionId: v.id("sessions"),
    role: v.union(v.literal("user"), v.literal("assistant"), v.literal("system")),
    content: v.string(),
    createdAt: v.number(),
  }).index("by_session", ["sessionId"]),

  // Host action intents enqueued for the Bun runtime to claim and execute.
  runtimeCommands: defineTable({
    kind: v.string(),
    payload: v.any(),
    status: v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("done"),
      v.literal("failed"),
    ),
    claimedBy: v.optional(v.string()),
    claimedAt: v.optional(v.number()),
    createdAt: v.number(),
  }).index("by_status", ["status"]),

  // Append-only observed host state, streamed back by the Bun runtime.
  runtimeEvents: defineTable({
    kind: v.string(),
    payload: v.any(),
    at: v.number(),
  }).index("by_kind", ["kind"]),

  // Latest heartbeat per runtime instance; drives online/offline in the UI.
  runtimeHeartbeats: defineTable({
    runtimeId: v.string(),
    at: v.number(),
  }).index("by_runtime", ["runtimeId"]),

  toolCalls: defineTable({
    sessionId: v.optional(v.id("sessions")),
    toolName: v.string(),
    args: v.any(),
    status: v.union(
      v.literal("queued"),
      v.literal("running"),
      v.literal("succeeded"),
      v.literal("failed"),
      v.literal("cancelled"),
    ),
    result: v.optional(v.any()),
    createdAt: v.number(),
  }).index("by_session", ["sessionId"]),
});
