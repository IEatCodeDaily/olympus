import type { CompletionSource } from "@codemirror/autocomplete";
import {
  findVaultSuggestion,
  serializeVaultSuggestion,
  type VaultSuggestion,
} from "./vaultMarkdown";

export function vaultCompletionSource(suggestions: VaultSuggestion[]): CompletionSource {
  return (context) => {
    const line = context.state.doc.lineAt(context.pos);
    const textBeforeCursor = line.text.slice(0, context.pos - line.from);
    const match = findVaultSuggestion(textBeforeCursor);
    if (!match) return null;

    const query = match.query.toLowerCase();
    const options = [];
    for (const suggestion of suggestions) {
      if (suggestion.kind !== match.kind) continue;
      if (
        !suggestion.label.toLowerCase().includes(query) &&
        !suggestion.id.toLowerCase().includes(query)
      ) continue;
      options.push({
        label: suggestion.label,
        detail: suggestion.id,
        type: "text",
        apply: serializeVaultSuggestion(suggestion),
      });
    }

    return {
      from: line.from + match.from,
      options,
      filter: false,
    };
  };
}
