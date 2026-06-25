/**
 * AgentRuntime — the stable boundary between Olympus and its execution engine.
 *
 * React and Convex MUST NOT import Hermes internals. All host execution flows
 * through this interface. The initial implementation is HermesAgentRuntime;
 * future implementations (OlympusBunRuntime, ConvexNativeAgentRuntime) can
 * replace it without touching the product model.
 */

import type { AgentRunStartCommand } from "@olympus/protocol";

export interface ToolDescriptor {
  name: string;
  description: string;
}

export interface ToolCallRequest {
  toolName: string;
  args: Record<string, unknown>;
}

export interface ToolCallResult {
  ok: boolean;
  output: unknown;
  error?: string;
}

export interface RunHandle {
  runId: string;
}

/** Emitted by the runtime as a run progresses. */
export type AgentRunEvent =
  | { kind: "started"; runId: string }
  | { kind: "delta"; runId: string; text: string }
  | { kind: "completed"; runId: string }
  | { kind: "failed"; runId: string; error: string };

export interface AgentRuntime {
  /** Start an agent run; resolves with a run handle once accepted. */
  startRun(command: AgentRunStartCommand): Promise<RunHandle>;
  /** Abort a running agent run. */
  abortRun(runId: string): Promise<void>;
  /** Stream run events as they occur. */
  streamEvents(runId: string): AsyncIterable<AgentRunEvent>;
  /** List tools available to a profile. */
  listTools(profileId: string): Promise<ToolDescriptor[]>;
  /** Invoke a single tool. */
  callTool(call: ToolCallRequest): Promise<ToolCallResult>;
}
