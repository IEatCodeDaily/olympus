import { useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { markdown as markdownLanguage } from "@codemirror/lang-markdown";
import { oneDark } from "@codemirror/theme-one-dark";
import { MilkdownRichEditor } from "./MilkdownRichEditor";
import {
  joinVaultMarkdown,
  richEditorFallbackReason,
  splitVaultMarkdown,
  type VaultSuggestion,
} from "./vaultMarkdown";

type EditorMode = "rich" | "source";

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
  const sourceOnlyReason = richEditorFallbackReason(markdown);
  const [mode, setMode] = useState<EditorMode>(sourceOnlyReason ? "source" : "rich");
  const effectiveMode = sourceOnlyReason ? "source" : mode;
  const document = useMemo(() => splitVaultMarkdown(markdown), [markdown]);

  const handleBodyChange = (body: string) => {
    onChange(joinVaultMarkdown({ frontmatter: document.frontmatter, body }));
  };

  return (
    <div className="vault-markdown-editor">
      <div className="vault-editor-toolbar" aria-label="Editor mode">
        <button
          type="button"
          className={`btn ${effectiveMode === "rich" ? "on" : ""}`}
          disabled={sourceOnlyReason !== null}
          onClick={() => setMode("rich")}
        >
          Rich
        </button>
        <button
          type="button"
          className={`btn ${effectiveMode === "source" ? "on" : ""}`}
          onClick={() => setMode("source")}
        >
          Source
        </button>
      </div>

      {sourceOnlyReason && (
        <div className="vault-source-warning" data-testid="vault-source-warning">
          Rich editing is disabled because this note has {sourceOnlyReason}. Use exact source mode to preserve it safely.
        </div>
      )}

      {effectiveMode === "source" ? (
        <div className="vault-source-editor" data-testid="vsrc">
          <CodeMirror
            value={markdown}
            extensions={[markdownLanguage()]}
            theme={oneDark}
            minHeight="420px"
            onChange={onChange}
            basicSetup={{ lineNumbers: true, foldGutter: true }}
          />
        </div>
      ) : (
        <>
          {document.frontmatter && (
            <details className="vault-frontmatter">
              <summary>YAML frontmatter (edit in Source mode)</summary>
              <pre data-testid="vault-frontmatter">{document.frontmatter}</pre>
            </details>
          )}
          <MilkdownRichEditor
            markdown={document.body}
            onChange={handleBodyChange}
            suggestions={suggestions}
          />
        </>
      )}
    </div>
  );
}
