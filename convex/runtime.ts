import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

const HEARTBEAT_STALE_MS = 30_000;

/** Bun runtime calls this on a fixed interval to signal liveness. */
export const heartbeat = mutation({
  args: { runtimeId: v.string() },
  returns: v.null(),
  handler: async (ctx, args) => {
    const now = Date.now();
    const existing = await ctx.db
      .query("runtimeHeartbeats")
      .withIndex("by_runtime", (q) => q.eq("runtimeId", args.runtimeId))
      .unique();
    if (existing) {
      await ctx.db.patch(existing._id, { at: now });
    } else {
      await ctx.db.insert("runtimeHeartbeats", { runtimeId: args.runtimeId, at: now });
    }
    return null;
  },
});

/** UI subscribes to this to render runtime online/offline. */
export const status = query({
  args: {},
  returns: v.array(
    v.object({
      runtimeId: v.string(),
      at: v.number(),
      online: v.boolean(),
    }),
  ),
  handler: async (ctx) => {
    const now = Date.now();
    const rows = await ctx.db.query("runtimeHeartbeats").collect();
    return rows.map((r) => ({
      runtimeId: r.runtimeId,
      at: r.at,
      online: now - r.at < HEARTBEAT_STALE_MS,
    }));
  },
});
