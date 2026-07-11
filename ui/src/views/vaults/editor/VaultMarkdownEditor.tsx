import { useMemo } from "react";
import { autocompletion } from "@codemirror/autocomplete";
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
}

const EMPTY_SUGGESTIONS: VaultSuggestion[] = [];

export function VaultMarkdownEditor({
  markdown,
  onChange,
  suggestions = EMPTY_SUGGESTIONS,
}: VaultMarkdownEditorProps) {
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

  return (
    <div className="vault-markdown-editor">
      {conflicted && (
        <div className="vault-source-warning" data-testid="vault-source-warning">
          This note contains an unresolved jj conflict. Resolve the conflict markers before saving.
        </div>
      )}
      <div className="vault-source-editor" data-testid="vsrc">
        <CodeMirror
          value={markdown}
          extensions={extensions}
          minHeight="420px"
          onChange={onChange}
          placeholder="Write Markdown…"
          basicSetup={{ lineNumbers: false, foldGutter: false }}
        />
      </div>
    </div>
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
