import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  SourceMarkdownEditor as VaultMarkdownEditor,
  VaultMarkdownEditor as RichVaultMarkdownEditor,
} from "./VaultMarkdownEditor";
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
  it("defaults to rich editing and only switches to source through the overflow action", async () => {
    const { container } = render(<RichVaultMarkdownEditor markdown="# Rich" onChange={() => {}} />);

    await waitFor(() => expect(container.querySelectorAll(".ProseMirror")).toHaveLength(1), { timeout: 90_000 });
    expect(screen.queryByTestId("vsrc")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Note actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit source" }));
    expect(screen.getByTestId("vsrc")).toHaveTextContent("# Rich");
    fireEvent.click(screen.getByRole("button", { name: "Note actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit rich" }));
    await waitFor(() => expect(container.querySelectorAll(".ProseMirror")).toHaveLength(1), { timeout: 90_000 });
  }, 120_000);

  it("opens unresolved conflicts in source and offers no automatic rich surface", () => {
    render(<RichVaultMarkdownEditor markdown={conflicted} onChange={() => {}} />);
    expect(screen.getByTestId("vsrc")).toBeInTheDocument();
    expect(screen.getByTestId("vault-source-warning")).toBeInTheDocument();
  });

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
    const onCancel = vi.fn();
    render(
      <RichVaultMarkdownEditor markdown="Draft" onChange={() => {}} dirty onSave={onSave} onCancel={onCancel} onDelete={onDelete} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel edits" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete note" }));
    expect(onSave).toHaveBeenCalledOnce();
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onDelete).toHaveBeenCalledOnce();
    expect(screen.queryByText("Unsaved")).not.toBeInTheDocument();
  });

  it("Ctrl+Shift+E switches from rich to source mode without a menu click", async () => {
    const onEditorModeChange = vi.fn();
    const { container } = render(
      <RichVaultMarkdownEditor
        markdown="# Switch me"
        onChange={() => {}}
        onEditorModeChange={onEditorModeChange}
      />,
    );

    // Wait for the real Milkdown editor to mount
    await waitFor(
      () => expect(container.querySelectorAll(".ProseMirror")).toHaveLength(1),
      { timeout: 90_000 },
    );
    expect(screen.queryByTestId("vsrc")).not.toBeInTheDocument();

    // Fire Ctrl+Shift+E on window — the effect listens on window
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, shiftKey: true, bubbles: true }),
    );

    // Source editor must appear
    await waitFor(() => expect(screen.getByTestId("vsrc")).toBeInTheDocument());
    expect(screen.getByTestId("vsrc")).toHaveTextContent("# Switch me");
    expect(onEditorModeChange).toHaveBeenCalledWith("source");

    // Fire Ctrl+Shift+E again → back to rich
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await waitFor(
      () => expect(container.querySelectorAll(".ProseMirror")).toHaveLength(1),
      { timeout: 90_000 },
    );
    expect(screen.queryByTestId("vsrc")).not.toBeInTheDocument();
    expect(onEditorModeChange).toHaveBeenCalledWith("rich");
  }, 120_000);

  it("Ctrl+Shift+E on a conflicted note stays in source and does not toggle", () => {
    const onEditorModeChange = vi.fn();
    render(
      <RichVaultMarkdownEditor
        markdown={conflicted}
        onChange={() => {}}
        onEditorModeChange={onEditorModeChange}
      />,
    );
    // Already in source because of conflict
    expect(screen.getByTestId("vsrc")).toBeInTheDocument();

    // Ctrl+Shift+E on a conflicted note must stay source
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, shiftKey: true, bubbles: true }),
    );

    // Still in source — no rich toggle when conflicted
    expect(screen.getByTestId("vsrc")).toBeInTheDocument();
  });

  it("Cancel button is disabled until the note has unsaved changes", () => {
    const onCancel = vi.fn();
    // The Cancel button lives in VaultMarkdownEditor's mode-actions toolbar,
    // which renders synchronously before Milkdown loads (outside the Suspense).
    render(
      <RichVaultMarkdownEditor markdown="Draft" onChange={() => {}} dirty={false} onCancel={onCancel} />,
    );
    const cancel = screen.getByRole("button", { name: "Cancel edits" });
    expect(cancel).toBeDisabled();
  }, 30_000);

  it("Save button is disabled until the note has unsaved changes", () => {
    const onSave = vi.fn();
    render(
      <RichVaultMarkdownEditor markdown="Draft" onChange={() => {}} dirty={false} onSave={onSave} />,
    );
    const save = screen.getByRole("button", { name: "Save note" });
    expect(save).toBeDisabled();
  }, 30_000);
});
