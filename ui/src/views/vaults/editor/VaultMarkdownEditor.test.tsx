import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { VaultMarkdownEditor } from "./VaultMarkdownEditor";
import { vaultCompletionSource } from "./vaultCompletion";

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

    expect(screen.getByTestId("vsrc")).toHaveTextContent("# Stable");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("edits the complete Markdown document in one lossless surface", () => {
    const markdown = "---\ntitle: Vault\n---\n# Editable body";
    render(
      <VaultMarkdownEditor
        markdown={markdown}
        onChange={() => {}}
      />,
    );

    expect(screen.getByTestId("vsrc")).toHaveTextContent("title: Vault");
    expect(screen.getByTestId("vsrc")).toHaveTextContent("Editable body");
    const editor = EditorView.findFromDOM(screen.getByTestId("vsrc").querySelector(".cm-editor")!)!;
    expect(editor.state.doc.toString()).toBe(markdown);
    expect(screen.queryByRole("button", { name: "Rich" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Source" })).not.toBeInTheDocument();
  });

  it("keeps conflicted notes editable without switching editor modes", () => {
    render(<VaultMarkdownEditor markdown={conflicted} onChange={() => {}} />);

    expect(screen.getByTestId("vault-source-warning")).toHaveTextContent(
      "unresolved jj conflict",
    );
    const lines = Array.from(
      screen.getByTestId("vsrc").querySelectorAll(".cm-line"),
      (line) => line.textContent ?? "",
    );
    expect(lines.join("\n")).toBe(conflicted);
    expect(screen.queryByRole("button", { name: "Rich" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Source" })).not.toBeInTheDocument();
  });

  it.each([
    "# Note\n\n<Component />",
    "# Note\n\n<!-- keep this -->",
    "Footnote[^1]\n\n[^1]: detail",
    "Read [the design][design].\n\n[design]: /design.md",
  ])("keeps extended Markdown editable in the same surface: %s", (markdown) => {
    render(<VaultMarkdownEditor markdown={markdown} onChange={() => {}} />);

    expect(screen.getByTestId("vsrc")).toHaveTextContent(markdown.split("\n")[0]);
    expect(screen.queryByTestId("vault-source-warning")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Rich" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Source" })).not.toBeInTheDocument();
  });

  it("offers Vault links through native editor completion", async () => {
    const state = EditorState.create({ doc: "See [[des" });
    const source = vaultCompletionSource([
      { kind: "note", id: "docs/design.md", label: "System design" },
      { kind: "note", id: "runbook.md", label: "Runbook" },
    ]);

    const result = await source(new CompletionContext(state, state.doc.length, true));

    expect(result?.from).toBe(4);
    expect(result?.options).toEqual([
      expect.objectContaining({ label: "System design", detail: "docs/design.md" }),
    ]);
  });

  it("renders inactive Markdown lines as a live preview without changing the document", async () => {
    render(<VaultMarkdownEditor markdown={"# Heading\n\n**bold**"} onChange={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("vsrc").querySelector(".vault-md-h1")).not.toBeNull();
      expect(screen.getByTestId("vsrc").querySelector(".vault-md-hidden-mark")).not.toBeNull();
    });
  });

  it("wraps long Markdown lines instead of creating a horizontal editor viewport", () => {
    render(<VaultMarkdownEditor markdown={"A very long line"} onChange={() => {}} />);

    expect(screen.getByTestId("vsrc").querySelector(".cm-lineWrapping")).not.toBeNull();
  });

  it("renders canonical wikilinks as links away from the active line", () => {
    const markdown = "# Note\n\nSee [[docs/design.md|System design]].";
    render(<VaultMarkdownEditor markdown={markdown} onChange={() => {}} />);

    expect(screen.getByTestId("vsrc").querySelector(".vault-md-wikilink")).toHaveTextContent("System design");
    const editor = EditorView.findFromDOM(screen.getByTestId("vsrc").querySelector(".cm-editor")!)!;
    expect(editor.state.doc.toString()).toBe(markdown);
  });

  it("fills the note surface and exposes native formatting controls", () => {
    render(<VaultMarkdownEditor markdown="Draft" onChange={() => {}} />);

    expect(screen.getByTestId("vsrc")).toHaveClass("vault-editor-canvas");
    expect(screen.getByRole("toolbar", { name: "Note formatting" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Italic" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Insert link" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bulleted list" })).toBeInTheDocument();
  });

  it("formats the current selection without replacing the editor", () => {
    const onChange = vi.fn();
    render(<VaultMarkdownEditor markdown="Draft" onChange={onChange} />);
    const editor = EditorView.findFromDOM(screen.getByTestId("vsrc").querySelector(".cm-editor")!)!;
    editor.dispatch({ selection: { anchor: 0, head: 5 } });

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    expect(editor.state.doc.toString()).toBe("**Draft**");
    expect(onChange.mock.lastCall?.[0]).toBe("**Draft**");
  });

  it("keeps save and delete actions in the editor toolbar", () => {
    const onSave = vi.fn();
    const onDelete = vi.fn();
    render(
      <VaultMarkdownEditor markdown="Draft" onChange={() => {}} dirty onSave={onSave} onDelete={onDelete} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete note" }));
    expect(onSave).toHaveBeenCalledOnce();
    expect(onDelete).toHaveBeenCalledOnce();
    expect(screen.queryByText("Unsaved")).not.toBeInTheDocument();
  });
});
