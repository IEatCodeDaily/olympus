/**
 * HermesAgentRuntime — initial AgentRuntime implementation backed by the
 * existing Hermes Agent. This is intentionally a thin placeholder for the
 * first milestone (runtime online + heartbeat). The real implementation will
 * shell out to the Hermes CLI / bridge socket; that wiring lands in a later task
 * (see docs/plans) once the command-claim loop and Convex schema are proven.
 */

import type { AgentRunStartCommand } from "@olympus/protocol";
import type {
  AgentRunEvent,
  AgentRuntime,
  RunHandle,
  ToolCallRequest,
  ToolCallResult,
  ToolDescriptor,
} from "./types";

export interface HermesAgentRuntimeOptions {
  /** Profile to run Hermes under (e.g. "default"). */
  profile?: string;
  /** Hermes CLI binary name/path. Defaults to "hermes". */
  hermesBin?: string;
}

export class HermesAgentRuntime implements AgentRuntime {
  private readonly profile: string;
  private readonly hermesBin: string;

  constructor(options: HermesAgentRuntimeOptions = {}) {
    this.profile = options.profile ?? "default";
    this.hermesBin = options.hermesBin ?? "hermes";
  }

  // NOTE: stubbed for milestone 1. Real implementation wires the Hermes bridge.
  async startRun(_command: AgentRunStartCommand): Promise<RunHandle> {
    throw new Error("HermesAgentRuntime.startRun not implemented yet");
  }

  async abortRun(_runId: string): Promise<void> {
    throw new Error("HermesAgentRuntime.abortRun not implemented yet");
  }

  // eslint-disable-next-line require-yield -- stub; real impl yields run events (milestone 2)
  async *streamEvents(_runId: string): AsyncIterable<AgentRunEvent> {
    throw new Error("HermesAgentRuntime.streamEvents not implemented yet");
  }

  async listTools(_profileId: string): Promise<ToolDescriptor[]> {
    throw new Error("HermesAgentRuntime.listTools not implemented yet");
  }

  async callTool(_call: ToolCallRequest): Promise<ToolCallResult> {
    throw new Error("HermesAgentRuntime.callTool not implemented yet");
  }

  /** Detection signal used by the runtime health check / UI. */
  describe(): { engine: "hermes"; profile: string; bin: string } {
    return { engine: "hermes", profile: this.profile, bin: this.hermesBin };
  }
}
