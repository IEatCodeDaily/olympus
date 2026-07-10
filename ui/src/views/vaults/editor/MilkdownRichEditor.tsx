import { useEffect, useRef, useState } from "react";
import { Crepe, CrepeFeature } from "@milkdown/crepe";
import { editorViewCtx } from "@milkdown/kit/core";
import { replaceRange } from "@milkdown/kit/utils";
import "@milkdown/crepe/theme/common/style.css";
import "@milkdown/crepe/theme/frame-dark.css";
import {
  findVaultSuggestion,
  fromRichMarkdown,
  serializeVaultSuggestion,
  toRichMarkdown,
  type VaultSuggestion,
  type VaultSuggestionMatch,
} from "./vaultMarkdown";

interface MilkdownRichEditorProps {
  markdown: string;
  onChange: (markdown: string) => void;
  suggestions?: VaultSuggestion[];
}

const EMPTY_SUGGESTIONS: VaultSuggestion[] = [];

export function MilkdownRichEditor({
  markdown,
  onChange,
  suggestions = EMPTY_SUGGESTIONS,
}: MilkdownRichEditorProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const crepeRef = useRef<Crepe | null>(null);
  const lifecycleRef = useRef<Promise<void>>(Promise.resolve());
  const onChangeRef = useRef(onChange);
  const initialMarkdown = useRef<string | undefined>(undefined);
  if (initialMarkdown.current === undefined) {
    initialMarkdown.current = toRichMarkdown(markdown);
  }
  const [activeMatch, setActiveMatch] = useState<VaultSuggestionMatch | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);

  onChangeRef.current = onChange;

  useEffect(() => {
    if (!rootRef.current) return;

    let disposed = false;
    const crepe = new Crepe({
      root: rootRef.current,
      defaultValue: initialMarkdown.current,
      features: {
        [CrepeFeature.AI]: false,
      },
      featureConfigs: {
        [CrepeFeature.Placeholder]: { text: "Write Markdown…" },
      },
    });

    crepe.on((listener) => {
      listener.markdownUpdated((ctx, nextMarkdown, previousMarkdown) => {
        if (disposed || nextMarkdown === previousMarkdown) return;
        const canonicalMarkdown = fromRichMarkdown(nextMarkdown);
        const view = ctx.get(editorViewCtx);
        const { $from } = view.state.selection;
        const textBeforeCursor = $from.parent.textBetween(0, $from.parentOffset, "\n", "\n");
        const match = findVaultSuggestion(textBeforeCursor);
        setActiveMatch(match);
        if (match) setSelectedIndex(0);
        onChangeRef.current(canonicalMarkdown);
      });
    });

    const ready = lifecycleRef.current.then(async () => {
      await crepe.create();
      if (!disposed) crepeRef.current = crepe;
    });

    return () => {
      disposed = true;
      lifecycleRef.current = ready.then(async () => {
        if (crepeRef.current === crepe) crepeRef.current = null;
        await crepe.destroy();
      });
    };
  }, []);

  const matches = activeMatch
    ? suggestions.filter(
        (item) =>
          item.kind === activeMatch.kind &&
          (item.label.toLowerCase().includes(activeMatch.query.toLowerCase()) ||
            item.id.toLowerCase().includes(activeMatch.query.toLowerCase())),
      )
    : [];

  const chooseSuggestion = (suggestion: VaultSuggestion) => {
    const crepe = crepeRef.current;
    if (!crepe || !activeMatch) return;

    const triggerLength = activeMatch.kind === "note" ? 2 : 1;
    crepe.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx);
      const to = view.state.selection.to;
      const from = to - triggerLength - activeMatch.query.length;
      const markdown = serializeVaultSuggestion(suggestion);
      if (suggestion.kind === "note") {
        view.dispatch(view.state.tr.insertText(markdown, from, to));
      } else {
        replaceRange(toRichMarkdown(markdown), { from, to })(ctx);
      }
      view.focus();
    });
    onChangeRef.current(fromRichMarkdown(crepe.getMarkdown()));
    setActiveMatch(null);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!activeMatch) return;
    if (event.key === "Escape") {
      event.preventDefault();
      setActiveMatch(null);
      return;
    }
    if (matches.length === 0) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setSelectedIndex((current) => (current + direction + matches.length) % matches.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      chooseSuggestion(matches[selectedIndex] ?? matches[0]);
    }
  };

  return (
    <div
      className="vault-rich-editor"
      data-testid="vault-rich-editor"
      onKeyDownCapture={handleKeyDown}
    >
      <div ref={rootRef} />
      {activeMatch && (
        <div className="vault-suggestions" role="listbox" aria-label={`${activeMatch.kind} suggestions`}>
          {matches.length > 0 ? (
            matches.map((suggestion, index) => (
              <button
                key={`${suggestion.kind}:${suggestion.id}`}
                type="button"
                role="option"
                aria-selected={index === selectedIndex}
                className={`vault-suggestion ${index === selectedIndex ? "selected" : ""}`}
                onMouseDown={(event) => {
                  event.preventDefault();
                  chooseSuggestion(suggestion);
                }}
              >
                <span>{suggestion.label}</span>
                <span className="mono">{suggestion.id}</span>
              </button>
            ))
          ) : (
            <div className="vault-suggestion-empty">No matching {activeMatch.kind}s</div>
          )}
        </div>
      )}
    </div>
  );
}
