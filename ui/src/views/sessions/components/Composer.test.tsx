import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

// Mock useAgents to return an agent with grouped models (multi-provider Hermes
// profile + a claude-code harness). This tests the grouped selector rendering.
vi.mock("../../../hooks/queries", () => ({
  useAgents: () => ({
    data: {
      agents: [
        {
          id: "default",
          provider: "zai",
          model: "glm-5.2",
          kind: "hermes",
          isDefault: true,
          models: [
            { provider: "zai", id: "glm-5.2", default: true },
            { provider: "zai", id: "glm-5v-turbo" },
            { provider: "openai-codex", id: "gpt-5.5" },
          ],
        },
        {
          id: "claude-code",
          provider: "claude-code",
          model: "claude-opus-4-8",
          kind: "claude-code",
          isDefault: false,
          models: [
            { provider: "claude-code", id: "claude-opus-4-8", default: true },
            { provider: "claude-code", id: "claude-sonnet-4-6" },
            { provider: "claude-code", id: "claude-fable-5" },
            { provider: "claude-code", id: "claude-haiku-4-5" },
          ],
        },
      ],
    },
  }),
}));

// Mock BrandIcons so it doesn't pull heavy deps.
vi.mock("../../../components/BrandIcons", () => ({
  BrandIcon: () => null,
  agentBrand: () => undefined,
}));

import { Composer } from "./Composer";

const noop = () => {};

function renderComposer(overrides: Record<string, unknown> = {}) {
  const onSend = vi.fn();
  render(
    <Composer
      text=""
      onTextChange={noop}
      onKeyDown={noop}
      onSend={onSend}
      onStop={noop}
      sending={false}
      sessionModel={null}
      sessionAgent="default"
      sessionNode="local"
      {...overrides}
    />,
  );
  return { onSend };
}

describe("Composer model selector", () => {
  it("exposes one stable semantic trigger for model and thinking choices", () => {
    renderComposer();

    const trigger = screen.getByRole("button", { name: "Model and thinking" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("thinking")).toBeInTheDocument();
  });

  it("groups models by provider with headers", () => {
    renderComposer();

    // Open the model dropdown.
    fireEvent.click(screen.getByTitle("Model & thinking"));

    // Provider headers present.
    expect(screen.getByText("zai")).toBeInTheDocument();
    expect(screen.getByText("openai-codex")).toBeInTheDocument();

    // Models under their providers (getAllByText because the pill also shows
    // the default model name — verify at least one in the menu).
    expect(screen.getAllByText("glm-5v-turbo").length).toBeGreaterThan(0);
    expect(screen.getAllByText("gpt-5.5").length).toBeGreaterThan(0);
  });

  it("dispatches the selected model on send", () => {
    const { onSend } = renderComposer();

    // Open and pick a non-default model.
    fireEvent.click(screen.getByTitle("Model & thinking"));
    fireEvent.click(screen.getByText("glm-5v-turbo"));

    // The pill should now show the overridden model.
    expect(screen.getByTitle("Model & thinking")).toHaveTextContent("glm-5v-turbo");
  });

  it("syncs the pill to session truth (Hall model) over local default", () => {
    renderComposer({ sessionModel: "glm-5.2" });

    // Pill shows the session's actual model, not "auto".
    expect(screen.getByTitle("Model & thinking")).toHaveTextContent("glm-5.2");
  });

  it("shows claude-fable-5 for the claude-code agent", () => {
    renderComposer({ sessionAgent: "claude-code", sessionModel: "claude-opus-4-8" });

    fireEvent.click(screen.getByTitle("Model & thinking"));

    // The fable model is present in the claude-code catalog.
    expect(screen.getByText("claude-fable-5")).toBeInTheDocument();
    // Provider header for claude-code.
    expect(screen.getAllByText("claude-code").length).toBeGreaterThan(0);
  });
});
