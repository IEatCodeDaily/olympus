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

export function toRichMarkdown(markdown: string): string {
  return transformMarkdownProse(markdown, (prose) =>
    prose.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (source, path, alias) => {
      const label = escapeMarkdownLabel(alias ?? noteLabel(path));
      return `[${label}](olympus-wikilink:${encodeURIComponent(source)})`;
    }),
  );
}

export function fromRichMarkdown(markdown: string): string {
  return transformMarkdownProse(markdown, (prose) =>
    prose
      .replace(
        /\[(?:\\.|[^\]])*\]\(olympus-wikilink:([^)]+)\)/g,
        (link, encodedSource) => safeDecodeWikilink(link, encodedSource),
      )
      .replace(/\\\[\\\[([^\]\n]+)(?:\\\]\\\]|\]\])/g, "[[$1]]"),
  );
}

export function richEditorFallbackReason(markdown: string): string | null {
  if (hasJjConflictMarkers(markdown)) return "unresolved jj conflict";

  const startsWithFrontmatter = markdown.startsWith("---\n") || markdown.startsWith("---\r\n");
  const document = splitVaultMarkdown(markdown);
  if (startsWithFrontmatter && !document.frontmatter) return "malformed YAML frontmatter";

  const prose = maskCode(document.body);
  const unsupported = [
    /^\s*:{2,}[A-Za-z][^\n]*$/m,
    /<\/?[A-Za-z][^>\n]*>/,
    /<!--[\s\S]*?-->/,
    /\[\^[^\]]+\]/,
    /^\s{0,3}\[[^\]\n]+\]:\s+\S+/m,
    /^\s*(?:import|export)\s/m,
  ];
  return unsupported.some((pattern) => pattern.test(prose))
    ? "unsupported Markdown syntax"
    : null;
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

function maskCode(markdown: string): string {
  let fence: string | null = null;
  return markdown
    .split(/(\r?\n)/)
    .map((part) => {
      if (/^\r?\n$/.test(part)) return part;
      const marker = /^\s*(`{3,}|~{3,})/.exec(part)?.[1] ?? null;
      if (marker && !fence) {
        fence = marker[0];
        return "";
      }
      if (marker && fence === marker[0]) {
        fence = null;
        return "";
      }
      return fence ? "" : part.replace(/`+[^`\n]*`+/g, "");
    })
    .join("");
}

function escapeMarkdownLabel(label: string): string {
  return label.replace(/([\\\]])/g, "\\$1");
}

function safeDecodeWikilink(link: string, encodedSource: string): string {
  try {
    const source = decodeURIComponent(encodedSource);
    return /^\[\[[^\n]+\]\]$/.test(source) ? source : link;
  } catch {
    return link;
  }
}

function transformMarkdownProse(markdown: string, transform: (prose: string) => string): string {
  let fence: { marker: string; length: number } | null = null;
  return markdown.replace(/[^\r\n]*(?:\r\n|\n|$)/g, (line) => {
    if (!line) return line;
    const content = line.replace(/\r?\n$/, "");
    const newline = line.slice(content.length);
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(content)?.[1];

    if (fence) {
      if (
        marker?.[0] === fence.marker &&
        marker.length >= fence.length &&
        new RegExp(`^ {0,3}${fence.marker}{${fence.length},}\\s*$`).test(content)
      ) {
        fence = null;
      }
      return line;
    }
    if (marker) {
      fence = { marker: marker[0], length: marker.length };
      return line;
    }
    if (/^(?: {4}|\t)/.test(content)) return line;

    return transformInlineProse(content, transform) + newline;
  });
}

function transformInlineProse(line: string, transform: (prose: string) => string): string {
  let result = "";
  let proseStart = 0;
  let cursor = 0;
  while (cursor < line.length) {
    if (line[cursor] !== "`" || (cursor > 0 && line[cursor - 1] === "\\")) {
      cursor += 1;
      continue;
    }
    let runEnd = cursor + 1;
    while (line[runEnd] === "`") runEnd += 1;
    const delimiter = line.slice(cursor, runEnd);
    const close = line.indexOf(delimiter, runEnd);
    if (close < 0) {
      cursor = runEnd;
      continue;
    }
    result += transform(line.slice(proseStart, cursor));
    result += line.slice(cursor, close + delimiter.length);
    cursor = close + delimiter.length;
    proseStart = cursor;
  }
  return result + transform(line.slice(proseStart));
}
