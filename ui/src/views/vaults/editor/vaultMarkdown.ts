export type VaultSuggestionKind = "mention" | "label" | "note";

export interface VaultSuggestionMatch {
  kind: VaultSuggestionKind;
  query: string;
  from: number;
  to: number;
}

export interface VaultSuggestion {
  kind: VaultSuggestionKind;
  id: string;
  label: string;
}

export interface VaultMarkdownDocument {
  frontmatter: string;
  body: string;
}

const CONFLICT_START = /^<<<<<<<(?: .*)?$/m;
const CONFLICT_MIDDLE = /^=======$/m;
const CONFLICT_END = /^>>>>>>>(?: .*)?$/m;

export function splitVaultMarkdown(markdown: string): VaultMarkdownDocument {
  if (!markdown.startsWith("---\n") && !markdown.startsWith("---\r\n")) {
    return { frontmatter: "", body: markdown };
  }

  const closingFence = /(?:\r?\n)---(?:\r?\n|$)/g;
  closingFence.lastIndex = markdown.startsWith("---\r\n") ? 5 : 4;
  const match = closingFence.exec(markdown);
  if (!match) return { frontmatter: "", body: markdown };

  const splitAt = match.index + match[0].length;
  return {
    frontmatter: markdown.slice(0, splitAt),
    body: markdown.slice(splitAt),
  };
}

export function joinVaultMarkdown(document: VaultMarkdownDocument): string {
  return document.frontmatter + document.body;
}


export function collectVaultSuggestions(
  markdown: string,
  linkedNotes: string[] = [],
): VaultSuggestion[] {
  const suggestions = new Map<string, VaultSuggestion>();
  const add = (suggestion: VaultSuggestion) => {
    suggestions.set(`${suggestion.kind}:${suggestion.id}`, suggestion);
  };

  for (const match of markdown.matchAll(
    /\[@([^\]]+)\]\(olympus:\/\/principal\/([^)]+)\)/g,
  )) {
    add({ kind: "mention", id: match[2], label: match[1] });
  }
  for (const match of markdown.matchAll(/(?:^|\s)#([\w/-]+)/gm)) {
    add({ kind: "label", id: match[1], label: match[1] });
  }
  for (const match of markdown.matchAll(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g)) {
    add({ kind: "note", id: match[1], label: match[2] ?? noteLabel(match[1]) });
  }
  for (const path of linkedNotes) {
    add({ kind: "note", id: path, label: noteLabel(path) });
  }

  return [...suggestions.values()];
}

export function hasJjConflictMarkers(markdown: string): boolean {
  return (
    CONFLICT_START.test(markdown) &&
    CONFLICT_MIDDLE.test(markdown) &&
    CONFLICT_END.test(markdown)
  );
}

export function findVaultSuggestion(textBeforeCursor: string): VaultSuggestionMatch | null {
  if (isInsideInlineCode(textBeforeCursor)) return null;

  const candidates: Array<{
    kind: VaultSuggestionKind;
    pattern: RegExp;
    prefixLength: number;
  }> = [
    { kind: "note", pattern: /(?:^|\s)\[\[([^\]\n]*)$/, prefixLength: 2 },
    { kind: "mention", pattern: /(?:^|\s)@([\w.-]*)$/, prefixLength: 1 },
    { kind: "label", pattern: /(?:^|\s)#([\w/-]*)$/, prefixLength: 1 },
  ];

  for (const candidate of candidates) {
    const match = candidate.pattern.exec(textBeforeCursor);
    if (!match) continue;

    const query = match[1] ?? "";
    const from = textBeforeCursor.length - query.length - candidate.prefixLength;
    return {
      kind: candidate.kind,
      query,
      from,
      to: textBeforeCursor.length,
    };
  }

  return null;
}

export function serializeVaultSuggestion(item: VaultSuggestion): string {
  switch (item.kind) {
    case "mention":
      return `[@${item.label}](olympus://principal/${item.id})`;
    case "label":
      return `#${item.id}`;
    case "note":
      return `[[${item.id}|${item.label}]]`;
  }
}

function isInsideInlineCode(text: string): boolean {
  const currentLine = text.slice(text.lastIndexOf("\n") + 1);
  const unescapedTicks = currentLine.match(/(?<!\\)`/g)?.length ?? 0;
  return unescapedTicks % 2 === 1;
}

function noteLabel(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.md$/, "");
}
