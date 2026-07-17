import type React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentPicker, deriveOftenSelectedPairs, nodeAgentRows } from "./AgentPicker";
import type { AgentInfo, NodeInfo, Session } from "../../../types";

function renderWithQuery(ui: React.ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

const agent = (id: string, provider = "anthropic"): AgentInfo => ({
  id,
  provider,
  model: provider === "anthropic" ? "claude-sonnet-4-6" : "gpt-5.5",
  kind: "hermes",
  isDefault: id === "default",
});

const node = (nodeId: string, status: NodeInfo["status"], agents: AgentInfo[]): NodeInfo => ({
  nodeId,
  hostname: `${nodeId}.host`,
  status,
  slotsUsed: 0,
  slotsTotal: 4,
  version: "0.1",
  local: nodeId === "local",
  lastHeartbeatAgoSecs: status === "online" ? 1 : 120,
  transport: nodeId === "local" ? "local" : "iroh",
  agents,
});

const session = (id: string, agentId: string | null, nodeId: string | null, lastActivity: number): Session => ({
  id,
  hermesId: id,
  orgId: "personal",
  ownerId: "rpw",
  contextId: null,
  source: "olympus",
  model: null,
  title: null,
  startedAt: lastActivity,
  lastActivity,
  messageCount: 0,
  inputTokens: 0,
  outputTokens: 0,
  archived: false,
  pinned: false,
  forkedFrom: null,
  forkPoint: null,
  forkType: null,
  managed: true,
  agent: agentId,
  node: nodeId,
  capabilities: null,
});

describe("AgentPicker helpers", () => {
  it("keeps duplicate agent ids as separate per-node rows", () => {
    const rows = nodeAgentRows([
      node("local", "online", [agent("default")]),
      node("fx-zephyrus", "online", [agent("default"), agent("codex", "openai-codex")]),
    ], "");

    expect(rows.map((row) => `${row.node.nodeId}:${row.agent.id}`)).toEqual([
      "local:default",
      "fx-zephyrus:default",
      "fx-zephyrus:codex",
    ]);
  });

  it("derives often-selected agent/node pairs by count with recency tiebreak", () => {
    const pairs = deriveOftenSelectedPairs([
      session("old-a", "default", "local", 10),
      session("new-b", "codex", "fx-zephyrus", 30),
      session("newer-a", "default", "local", 40),
      session("missing-node", "tester", null, 50),
    ], [
      node("local", "online", [agent("default")]),
      node("fx-zephyrus", "online", [agent("codex", "openai-codex")]),
    ]);

    expect(pairs.map((pair) => `${pair.node.nodeId}:${pair.agent.id}`)).toEqual([
      "local:default",
      "fx-zephyrus:codex",
    ]);
  });

  it("excludes offline nodes from often-selected", () => {
    const pairs = deriveOftenSelectedPairs([
      session("offline", "default", "edge-mini", 50),
    ], [node("edge-mini", "offline", [agent("default")])]);

    expect(pairs).toEqual([]);
  });
});

describe("AgentPicker", () => {
  it("groups by node, disables offline nodes, and returns the explicit node", () => {
    const onSelect = vi.fn();
    renderWithQuery(
      <AgentPicker
        open
        onSelect={onSelect}
        onCancel={() => {}}
        nodesOverride={[
          node("local", "online", [agent("default")]),
          node("edge-mini", "offline", [agent("tester")]),
        ]}
        sessionsOverride={[session("recent", "default", "local", 60)]}
      />,
    );

    fireEvent.click(screen.getAllByRole("button", { name: /default on local/i })[0]);
    expect(onSelect).toHaveBeenCalledWith("default", "local");

    const offline = screen.getByText(/edge-mini.host/).closest("section")!;
    expect(within(offline).queryByRole("button", { name: /tester/i })).toBeNull();
    expect(within(offline).getAllByText(/offline/i).length).toBeGreaterThan(0);
  });

  it("shows create errors without closing the picker", () => {
    renderWithQuery(
      <AgentPicker
        open
        error="Node edge-mini is Offline; choose an online node"
        onSelect={() => {}}
        onCancel={() => {}}
        nodesOverride={[node("local", "online", [agent("default")])]}
        sessionsOverride={[]}
      />,
    );

    expect(screen.getByRole("dialog", { name: /start new session/i })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(/edge-mini is Offline/i);
  });

  it("highlights keyboard selection in rendered order, including often-selected", () => {
    const onSelect = vi.fn();
    renderWithQuery(
      <AgentPicker
        open
        onSelect={onSelect}
        onCancel={() => {}}
        nodesOverride={[
          node("local", "online", [agent("default")]),
          node("fx-zephyrus", "online", [agent("codex", "openai-codex")]),
        ]}
        sessionsOverride={[session("recent", "codex", "fx-zephyrus", 60)]}
      />,
    );

    const buttons = screen.getAllByRole("button", { name: / on /i });
    expect(buttons[0]).toHaveAttribute("data-active", "true");

    fireEvent.keyDown(window, { key: "ArrowDown" });
    expect(buttons[1]).toHaveAttribute("data-active", "true");

    fireEvent.keyDown(window, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("default", "local");
  });
});
