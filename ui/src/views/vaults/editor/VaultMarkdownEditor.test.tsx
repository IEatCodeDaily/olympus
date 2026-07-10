import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { VaultMarkdownEditor } from "./VaultMarkdownEditor";

vi.mock("./MilkdownRichEditor", () => ({
  MilkdownRichEditor: ({ markdown }: { markdown: string }) => (
    <div data-testid="milkdown-rich">{markdown}</div>
  ),
}));

const conflicted = [
  "---",
  "title: Conflict",
  "---",
  "<<<<<<< working-copy",
  "human",
  "=======",
  "agent",
  ">>>>>>> revision",
].join("\n");

describe("VaultMarkdownEditor", () => {
  it("does not emit a change merely because it mounted", () => {
    const onChange = vi.fn();

    render(<VaultMarkdownEditor markdown="# Stable" onChange={onChange} />);

    expect(screen.getByTestId("milkdown-rich")).toHaveTextContent("# Stable");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps frontmatter outside the rich editing surface", () => {
    render(
      <VaultMarkdownEditor
        markdown={"---\ntitle: Vault\n---\n# Editable body"}
        onChange={() => {}}
      />,
    );

    expect(screen.getByTestId("vault-frontmatter")).toHaveTextContent("title: Vault");
    expect(screen.getByTestId("milkdown-rich")).toHaveTextContent("# Editable body");
  });

  it("forces conflicted notes into exact source mode", () => {
    render(<VaultMarkdownEditor markdown={conflicted} onChange={() => {}} />);

    expect(screen.queryByTestId("milkdown-rich")).not.toBeInTheDocument();
    expect(screen.getByTestId("vault-source-warning")).toHaveTextContent(
      "unresolved jj conflict",
    );
    const lines = Array.from(
      screen.getByTestId("vsrc").querySelectorAll(".cm-line"),
      (line) => line.textContent ?? "",
    );
    expect(lines.join("\n")).toBe(conflicted);
    expect(screen.getByRole("button", { name: "Rich" })).toBeDisabled();
  });

  it("fails closed to source mode for unsupported Markdown", () => {
    render(
      <VaultMarkdownEditor markdown={"# Note\n\n<Component />"} onChange={() => {}} />,
    );

    expect(screen.queryByTestId("milkdown-rich")).not.toBeInTheDocument();
    expect(screen.getByTestId("vault-source-warning")).toHaveTextContent(
      "unsupported Markdown syntax",
    );
    expect(screen.getByRole("button", { name: "Rich" })).toBeDisabled();
  });

  it("allows manual switching between rich and source modes", () => {
    render(<VaultMarkdownEditor markdown="# Draft" onChange={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.getByTestId("vsrc").querySelector(".cm-content")).toHaveTextContent(
      "# Draft",
    );

    fireEvent.click(screen.getByRole("button", { name: "Rich" }));
    expect(screen.getByTestId("milkdown-rich")).toBeInTheDocument();
  });
});
