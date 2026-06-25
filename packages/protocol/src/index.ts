/**
 * Shared runtime command/event protocol between React, Convex, and the Bun runtime.
 *
 * Kept dependency-free (no zod yet) so it imports cleanly in the Convex runtime,
 * the browser bundle, and Bun. Validation helpers are plain functions.
 */

export type RuntimeCommandKind =
  | "agent.run.start"
  | "agent.run.abort"
  | "terminal.open"
  | "terminal.input"
  | "fs.read";

export interface AgentRunStartCommand {
  kind: "agent.run.start";
  commandId: string;
  profileId: string;
  sessionId: string;
  input: string;
  provider?: string;
  model?: string;
}

export interface AgentRunAbortCommand {
  kind: "agent.run.abort";
  commandId: string;
  runId: string;
}

export interface TerminalOpenCommand {
  kind: "terminal.open";
  commandId: string;
  profileId: string;
  cwd?: string;
  shell?: string;
}

export interface TerminalInputCommand {
  kind: "terminal.input";
  commandId: string;
  terminalId: string;
  data: string;
}

export interface FsReadCommand {
  kind: "fs.read";
  commandId: string;
  profileId: string;
  path: string;
}

export type RuntimeCommand =
  | AgentRunStartCommand
  | AgentRunAbortCommand
  | TerminalOpenCommand
  | TerminalInputCommand
  | FsReadCommand;

export type RuntimeEvent =
  | { kind: "runtime.heartbeat"; runtimeId: string; at: number }
  | { kind: "command.claimed"; commandId: string; runtimeId: string; at: number }
  | { kind: "agent.run.started"; commandId: string; runId: string; at: number }
  | { kind: "agent.run.delta"; runId: string; text: string; at: number }
  | { kind: "agent.run.completed"; runId: string; at: number }
  | { kind: "agent.run.failed"; runId: string; error: string; at: number }
  | { kind: "terminal.output"; terminalId: string; data: string; at: number };

export const RUNTIME_COMMAND_KINDS: readonly RuntimeCommandKind[] = [
  "agent.run.start",
  "agent.run.abort",
  "terminal.open",
  "terminal.input",
  "fs.read",
] as const;

export function isRuntimeCommand(value: unknown): value is RuntimeCommand {
  if (typeof value !== "object" || value === null) return false;
  const kind = (value as { kind?: unknown }).kind;
  const commandId = (value as { commandId?: unknown }).commandId;
  return (
    typeof kind === "string" &&
    (RUNTIME_COMMAND_KINDS as readonly string[]).includes(kind) &&
    typeof commandId === "string" &&
    commandId.length > 0
  );
}

export function parseRuntimeCommand(value: unknown): RuntimeCommand {
  if (!isRuntimeCommand(value)) {
    throw new Error("Invalid RuntimeCommand payload");
  }
  return value;
}
