import { lazy, Suspense, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { autocompletion } from "@codemirror/autocomplete";
import { redo, undo } from "@codemirror/commands";
import { syntaxTree } from "@codemirror/language";
import type { Range } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from "@codemirror/view";
import CodeMirror from "@uiw/react-codemirror";
import { markdown as markdownLanguage } from "@codemirror/lang-markdown";
import { hasJjConflictMarkers, type VaultSuggestion } from "./vaultMarkdown";
import { vaultCompletionSource } from "./vaultCompletion";

interface VaultMarkdownEditorProps {
  markdown: string;
  onChange: (markdown: string) => void;
  suggestions?: VaultSuggestion[];
  dirty?: boolean;
  saving?: boolean;
  saveError?: string | null;
  onSave?: () => void;
  onCancel?: () => void;
  onDelete?: () => void;
  /** Initial mode override per-tab — only used at mount, not controlled. */
  editorMode?: "rich" | "source";
  /** Called whenever the user explicitly switches modes. */
  onEditorModeChange?: (mode: "rich" | "source") => void;
}

const EMPTY_SUGGESTIONS: VaultSuggestion[] = [];
const MilkdownRichEditor = lazy(() => import("./MilkdownRichEditor").then((module) => ({ default: module.MilkdownRichEditor })));

export function VaultMarkdownEditor(props: VaultMarkdownEditorProps) {
  const conflicted = hasJjConflictMarkers(props.markdown);
  const [mode, setMode] = useState<"rich" | "source">(() => {
    // Conflicts always start in source — no override possible.
    if (conflicted) return "source";
    // Honour the per-tab persisted mode, if provided.
    return props.editorMode ?? "rich";
  });
  const [menuOpen, setMenuOpen] = useState(false);
  const [editorGeneration, setEditorGeneration] = useState(0);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || !event.shiftKey || event.key.toLowerCase() !== "e") return;
      event.preventDefault();
      setMode((current) => {
        const next = current === "rich" ? "source" : (conflicted ? "source" : "rich");
        props.onEditorModeChange?.(next);
        return next;
      });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [conflicted]);

  return (
    <div className="vault-markdown-editor">
      <div className="vault-note-mode-actions">
        {props.onSave && <button type="button" className="btn pri" aria-label="Save note" disabled={props.saving || !props.dirty} onClick={props.onSave}>{props.saving ? "Saving…" : "Save"}</button>}
        {props.onCancel && <button type="button" className="vault-toolbar-button" aria-label="Cancel edits" disabled={props.saving || !props.dirty} onClick={() => { props.onCancel?.(); setEditorGeneration((generation) => generation + 1); }}>Cancel</button>}
        {props.onDelete && <button type="button" className="vault-toolbar-button danger" aria-label="Delete note" onClick={props.onDelete}>Delete</button>}
        <button type="button" className="vault-toolbar-button" aria-label="Note actions" aria-expanded={menuOpen} onClick={() => setMenuOpen((open) => !open)}>⋯</button>
        {menuOpen && (
          <div className="vault-note-menu" role="menu">
            <button type="button" role="menuitem" onClick={() => { const next = mode === "rich" ? "source" : "rich"; setMode(next); props.onEditorModeChange?.(next); setMenuOpen(false); }} disabled={conflicted && mode === "source"}>
              {mode === "rich" ? "Edit source" : "Edit rich"}
            </button>
          </div>
        )}
      </div>
      {props.saveError && <div className="vault-save-error" role="alert">{props.saveError}</div>}
      {mode === "rich" ? (
        <Suspense fallback={<div className="vault-editor-loading">Loading rich editor…</div>}>
          <MilkdownRichEditor key={editorGeneration} markdown={props.markdown} onChange={props.onChange} suggestions={props.suggestions} />
        </Suspense>
      ) : <SourceMarkdownEditor {...props} onSave={undefined} onDelete={undefined} saveError={null} />}
    </div>
  );
}

function preventToolbarBlur(event: ReactMouseEvent<HTMLButtonElement>) {
  event.preventDefault();
}

export function SourceMarkdownEditor({
  markdown,
  onChange,
  suggestions = EMPTY_SUGGESTIONS,
  dirty = false,
  saving = false,
  saveError = null,
  onSave,
  onDelete,
}: VaultMarkdownEditorProps) {
  const editorRef = useRef<EditorView | null>(null);
  const conflicted = hasJjConflictMarkers(markdown);
  const extensions = useMemo(
    () => [
      markdownLanguage(),
      EditorView.lineWrapping,
      ...(!conflicted ? [vaultLivePreview] : []),
      autocompletion({ override: [vaultCompletionSource(suggestions)] }),
    ],
    [conflicted, suggestions],
  );

  const wrapSelection = (prefix: string, suffix = prefix, placeholder = "text") => {
    const view = editorRef.current;
    if (!view) return;
    const selection = view.state.selection.main;
    const selected = view.state.sliceDoc(selection.from, selection.to) || placeholder;
    view.dispatch({
      changes: { from: selection.from, to: selection.to, insert: `${prefix}${selected}${suffix}` },
      selection: { anchor: selection.from + prefix.length, head: selection.from + prefix.length + selected.length },
    });
    view.focus();
  };

  const prefixLines = (prefix: string) => {
    const view = editorRef.current;
    if (!view) return;
    const selection = view.state.selection.main;
    const from = view.state.doc.lineAt(selection.from).from;
    const to = view.state.doc.lineAt(selection.to).to;
    const replacement = view.state.sliceDoc(from, to).split("\n").map((line) => `${prefix}${line}`).join("\n");
    view.dispatch({ changes: { from, to, insert: replacement } });
    view.focus();
  };

  return (
    <div className="vault-source-markdown-editor">
      <div className="vault-note-toolbar" role="toolbar" aria-label="Note formatting">
        <div className="vault-note-tools">
          <ToolbarButton label="Undo" text="↶" onMouseDown={preventToolbarBlur} onClick={() => editorRef.current && undo(editorRef.current)} />
          <ToolbarButton label="Redo" text="↷" onMouseDown={preventToolbarBlur} onClick={() => editorRef.current && redo(editorRef.current)} />
          <span className="vault-toolbar-divider" aria-hidden="true" />
          <ToolbarButton label="Heading" text="H" onMouseDown={preventToolbarBlur} onClick={() => prefixLines("# ")} />
          <ToolbarButton label="Bold" text="B" strong onMouseDown={preventToolbarBlur} onClick={() => wrapSelection("**")} />
          <ToolbarButton label="Italic" text="I" italic onMouseDown={preventToolbarBlur} onClick={() => wrapSelection("*")} />
          <ToolbarButton label="Strikethrough" text="S" strike onMouseDown={preventToolbarBlur} onClick={() => wrapSelection("~~")} />
          <ToolbarButton label="Inline code" text="<>" onMouseDown={preventToolbarBlur} onClick={() => wrapSelection("`")} />
          <ToolbarButton label="Insert link" text="Link" onMouseDown={preventToolbarBlur} onClick={() => wrapSelection("[", "](url)")} />
          <span className="vault-toolbar-divider" aria-hidden="true" />
          <ToolbarButton label="Bulleted list" text="• List" onMouseDown={preventToolbarBlur} onClick={() => prefixLines("- ")} />
          <ToolbarButton label="Numbered list" text="1. List" onMouseDown={preventToolbarBlur} onClick={() => prefixLines("1. ")} />
          <ToolbarButton label="Blockquote" text="Quote" onMouseDown={preventToolbarBlur} onClick={() => prefixLines("> ")} />
        </div>
        <div className="vault-note-actions">
          {onSave && <button type="button" className="btn pri" aria-label="Save note" disabled={saving || !dirty} onClick={onSave}>{saving ? "Saving…" : "Save"}</button>}
          {onDelete && <button type="button" className="vault-toolbar-button danger" aria-label="Delete note" onClick={onDelete}>Delete</button>}
        </div>
      </div>
      {conflicted && (
        <div className="vault-source-warning" data-testid="vault-source-warning">
          This note contains an unresolved jj conflict. Resolve the conflict markers before saving.
        </div>
      )}
      {saveError && <div className="vault-save-error" role="alert">{saveError}</div>}
      <div className="vault-source-editor vault-editor-canvas" data-testid="vsrc">
        <CodeMirror
          value={markdown}
          extensions={extensions}
          height="100%"
          onChange={onChange}
          placeholder="Write Markdown…"
          basicSetup={{ lineNumbers: false, foldGutter: false }}
          onCreateEditor={(view) => { editorRef.current = view; }}
        />
      </div>
    </div>
  );
}

interface ToolbarButtonProps {
  label: string;
  text: string;
  onClick: () => void;
  onMouseDown: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  strong?: boolean;
  italic?: boolean;
  strike?: boolean;
}

function ToolbarButton({ label, text, onClick, onMouseDown, strong, italic, strike }: ToolbarButtonProps) {
  const emphasis = [strong && "strong", italic && "italic", strike && "strike"].filter(Boolean).join(" ");
  return (
    <button
      type="button"
      className={`vault-toolbar-button ${emphasis}`}
      aria-label={label}
      title={label}
      onMouseDown={onMouseDown}
      onClick={onClick}
    >
      {text}
    </button>
  );
}

const MARK_NODES = new Set(["HeaderMark", "EmphasisMark", "LinkMark", "URL", "CodeMark"]);
const HEADING_CLASSES: Record<string, string> = {
  ATXHeading1: "vault-md-h1",
  ATXHeading2: "vault-md-h2",
  ATXHeading3: "vault-md-h3",
  ATXHeading4: "vault-md-h4",
  ATXHeading5: "vault-md-h5",
  ATXHeading6: "vault-md-h6",
};

class HiddenMarkWidget extends WidgetType {
  toDOM() {
    const element = document.createElement("span");
    element.className = "vault-md-hidden-mark";
    return element;
  }
}

class BulletWidget extends WidgetType {
  toDOM() {
    const element = document.createElement("span");
    element.className = "vault-md-bullet";
    element.textContent = "•";
    return element;
  }
}

function buildLivePreviewDecorations(view: EditorView): DecorationSet {
  const ranges: Array<Range<Decoration>> = [];
  const activeLine = view.state.doc.lineAt(view.state.selection.main.head);
  const frontmatterEnd = findFrontmatterEnd(view.state.doc.toString());

  syntaxTree(view.state).iterate({
    enter(node) {
      const line = view.state.doc.lineAt(node.from);
      const isActiveLine = line.number === activeLine.number;
      const inFrontmatter = frontmatterEnd > 0 && node.from < frontmatterEnd;
      const headingClass = HEADING_CLASSES[node.name];

      if (headingClass && !inFrontmatter) {
        ranges.push(Decoration.line({ class: headingClass }).range(line.from));
      } else if (node.name === "Blockquote") {
        ranges.push(Decoration.line({ class: "vault-md-blockquote" }).range(line.from));
      } else if (MARK_NODES.has(node.name) && !isActiveLine && !inFrontmatter) {
        ranges.push(Decoration.replace({ widget: new HiddenMarkWidget() }).range(node.from, node.to));
      } else if (node.name === "ListMark" && !isActiveLine) {
        ranges.push(Decoration.replace({ widget: new BulletWidget() }).range(node.from, node.to));
      }
    },
  });

  for (let lineNumber = 1; lineNumber <= view.state.doc.lines; lineNumber += 1) {
    const line = view.state.doc.line(lineNumber);
    if (line.number === activeLine.number || (frontmatterEnd > 0 && line.from < frontmatterEnd)) continue;
    for (const match of line.text.matchAll(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g)) {
      const source = match[0];
      const start = line.from + (match.index ?? 0);
      const end = start + source.length;
      const pipe = source.indexOf("|");
      const labelFrom = pipe >= 0 ? start + pipe + 1 : start + 2;
      const hiddenPrefixEnd = pipe >= 0 ? start + pipe + 1 : start + 2;
      ranges.push(Decoration.replace({ widget: new HiddenMarkWidget() }).range(start, hiddenPrefixEnd));
      ranges.push(Decoration.mark({ class: "vault-md-wikilink" }).range(labelFrom, end - 2));
      ranges.push(Decoration.replace({ widget: new HiddenMarkWidget() }).range(end - 2, end));
    }
  }

  return Decoration.set(ranges, true);
}

function findFrontmatterEnd(markdown: string): number {
  if (!markdown.startsWith("---\n") && !markdown.startsWith("---\r\n")) return 0;
  const closingFence = /(?:\r?\n)---(?:\r?\n|$)/g;
  closingFence.lastIndex = markdown.startsWith("---\r\n") ? 5 : 4;
  const match = closingFence.exec(markdown);
  return match ? match.index + match[0].length : 0;
}

const vaultLivePreview = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildLivePreviewDecorations(view);
    }

    update(update: { view: EditorView; docChanged: boolean; selectionSet: boolean; viewportChanged: boolean }) {
      if (update.docChanged || update.selectionSet || update.viewportChanged) {
        this.decorations = buildLivePreviewDecorations(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
