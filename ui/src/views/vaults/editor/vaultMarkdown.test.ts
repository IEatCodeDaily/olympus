import { describe, expect, it } from "vitest";
import {
  collectVaultSuggestions,
  findVaultSuggestion,
  fromRichMarkdown,
  hasJjConflictMarkers,
  joinVaultMarkdown,
  richEditorFallbackReason,
  serializeVaultSuggestion,
  splitVaultMarkdown,
  toRichMarkdown,
  type VaultSuggestion,
} from "./vaultMarkdown";

describe("Vault Markdown frontmatter boundary", () => {
  it("keeps YAML frontmatter byte-for-byte outside the rich editor", () => {
    const markdown = "---\r\ntitle: Vault\r\ncustom:\r\n  untouched: true\r\n---\r\n# Body\r\n";

    const document = splitVaultMarkdown(markdown);

    expect(document.frontmatter).toBe(
      "---\r\ntitle: Vault\r\ncustom:\r\n  untouched: true\r\n---\r\n",
    );
    expect(document.body).toBe("# Body\r\n");
    expect(joinVaultMarkdown(document)).toBe(markdown);
  });

  it("leaves ordinary thematic breaks in the editable body", () => {
    const markdown = "# Body\n\n---\n\nNext";

    expect(splitVaultMarkdown(markdown)).toEqual({ frontmatter: "", body: markdown });
  });
});

describe("collectVaultSuggestions", () => {
  it("reuses stable mentions, labels, and linked note targets", () => {
    expect(
      collectVaultSuggestions(
        "[@Terminus](olympus://principal/terminus) owns #architecture. See [[docs/design.md|Design]].",
        ["other.md"],
      ),
    ).toEqual([
      { kind: "mention", id: "terminus", label: "Terminus" },
      { kind: "label", id: "architecture", label: "architecture" },
      { kind: "note", id: "docs/design.md", label: "Design" },
      { kind: "note", id: "other.md", label: "other" },
    ]);
  });
});

describe("Milkdown wikilink bridge", () => {
  it("presents wikilinks as rich links and restores canonical syntax", () => {
    const canonical = "See [[docs/design.md|System design]] and [[quick-note.md]].";
    const rich = toRichMarkdown(canonical);

    expect(rich).toContain("[System design](olympus-wikilink:");
    expect(rich).toContain("[quick-note](olympus-wikilink:");
    expect(fromRichMarkdown(rich)).toBe(canonical);
    expect(fromRichMarkdown("See \\[\\[quick-note.md\\]\\].")).toBe(
      "See [[quick-note.md]].",
    );
    expect(fromRichMarkdown("See \\[\\[quick-note.md]].")).toBe("See [[quick-note.md]].");
  });
  it("does not rewrite wikilink-shaped text inside code", () => {
    const markdown = "Inline `[[literal.md]]`.\n\n```md\n[[fenced.md]]\n```\n\nSee [[real.md]].";
    const rich = toRichMarkdown(markdown);

    expect(rich).toContain("`[[literal.md]]`");
    expect(rich).toContain("```md\n[[fenced.md]]\n```");
    expect(rich).toContain("[real](olympus-wikilink:");
    expect(fromRichMarkdown(rich)).toBe(markdown);
  });
});

describe("richEditorFallbackReason", () => {
  it.each([
    ["---\ntitle: broken\n# body", "malformed YAML frontmatter"],
    ["# note\n\n:::custom\nvalue\n:::", "unsupported Markdown syntax"],
    ["# note\n\n<Component value={1} />", "unsupported Markdown syntax"],
    ["# note\n\n<!-- source-only comment -->", "unsupported Markdown syntax"],
    ["Read [the design][design].\n\n[design]: /design.md", "unsupported Markdown syntax"],
    ["# note\n\nFootnote[^1]\n\n[^1]: detail", "unsupported Markdown syntax"],
  ])("falls back for %s", (markdown, reason) => {
    expect(richEditorFallbackReason(markdown)).toBe(reason);
  });

  it("allows the supported canonical subset", () => {
    expect(
      richEditorFallbackReason(
        "---\ntitle: safe\n---\n# Body\n\n[[note.md]] [view](vault://notes.db?view=active)",
      ),
    ).toBeNull();
  });
});

describe("hasJjConflictMarkers", () => {
  it("detects a complete unresolved jj conflict", () => {
    const markdown = [
      "# Draft",
      "",
      "<<<<<<< working-copy",
      "human text",
      "=======",
      "agent text",
      ">>>>>>> revision",
    ].join("\n");

    expect(hasJjConflictMarkers(markdown)).toBe(true);
  });

  it("does not treat ordinary thematic breaks as conflicts", () => {
    expect(hasJjConflictMarkers("# Draft\n\n---\n\nReady")).toBe(false);
  });
});

describe("findVaultSuggestion", () => {
  it.each([
    ["Ask @term", "mention", "term"],
    ["Tagged #arch", "label", "arch"],
    ["See [[vault", "note", "vault"],
  ] as const)("finds %s", (source, kind, query) => {
    expect(findVaultSuggestion(source)).toMatchObject({ kind, query });
  });

  it("does not trigger labels for Markdown headings", () => {
    expect(findVaultSuggestion("# Architecture")).toBeNull();
  });

  it("does not trigger in inline code", () => {
    expect(findVaultSuggestion("Use `@term")).toBeNull();
  });

  it("does not trigger after an identifier character", () => {
    expect(findVaultSuggestion("email@example")).toBeNull();
  });
});

describe("serializeVaultSuggestion", () => {
  it.each([
    [
      { kind: "mention", id: "terminus", label: "Terminus" },
      "[@Terminus](olympus://principal/terminus)",
    ],
    [{ kind: "label", id: "architecture", label: "architecture" }, "#architecture"],
    [{ kind: "note", id: "docs/vault.md", label: "Vault" }, "[[docs/vault.md|Vault]]"],
  ] satisfies Array<[VaultSuggestion, string]>)("serializes %j", (item, expected) => {
    expect(serializeVaultSuggestion(item)).toBe(expected);
  });
});
