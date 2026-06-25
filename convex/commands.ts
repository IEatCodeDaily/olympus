import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

/** Enqueue a host action intent for the Bun runtime to claim. */
export const enqueue = mutation({
  args: { kind: v.string(), payload: v.any() },
  returns: v.id("runtimeCommands"),
  handler: async (ctx, args) => {
    return await ctx.db.insert("runtimeCommands", {
      kind: args.kind,
      payload: args.payload,
      status: "pending",
      createdAt: Date.now(),
    });
  },
});

/** Atomically claim the oldest pending command for a runtime instance. */
export const claimNext = mutation({
  args: { runtimeId: v.string() },
  returns: v.union(
    v.object({ _id: v.id("runtimeCommands"), kind: v.string(), payload: v.any() }),
    v.null(),
  ),
  handler: async (ctx, args) => {
    const next = await ctx.db
      .query("runtimeCommands")
      .withIndex("by_status", (q) => q.eq("status", "pending"))
      .order("asc")
      .first();
    if (!next) return null;
    await ctx.db.patch(next._id, {
      status: "claimed",
      claimedBy: args.runtimeId,
      claimedAt: Date.now(),
    });
    await ctx.db.insert("runtimeEvents", {
      kind: "command.claimed",
      payload: { commandId: next._id, runtimeId: args.runtimeId },
      at: Date.now(),
    });
    return { _id: next._id, kind: next.kind, payload: next.payload };
  },
});

/** Mark a claimed command done or failed. */
export const complete = mutation({
  args: {
    commandId: v.id("runtimeCommands"),
    status: v.union(v.literal("done"), v.literal("failed")),
  },
  returns: v.null(),
  handler: async (ctx, args) => {
    await ctx.db.patch(args.commandId, { status: args.status });
    return null;
  },
});

/** Bun runtime appends observed host state here. */
export const appendEvent = mutation({
  args: { kind: v.string(), payload: v.any() },
  returns: v.null(),
  handler: async (ctx, args) => {
    await ctx.db.insert("runtimeEvents", { kind: args.kind, payload: args.payload, at: Date.now() });
    return null;
  },
});

/** Recent events for the UI / debugging. */
export const recentEvents = query({
  args: { limit: v.optional(v.number()) },
  returns: v.array(
    v.object({
      _id: v.id("runtimeEvents"),
      kind: v.string(),
      payload: v.any(),
      at: v.number(),
    }),
  ),
  handler: async (ctx, args) => {
    const rows = await ctx.db.query("runtimeEvents").order("desc").take(args.limit ?? 50);
    return rows.map((r) => ({ _id: r._id, kind: r.kind, payload: r.payload, at: r.at }));
  },
});
