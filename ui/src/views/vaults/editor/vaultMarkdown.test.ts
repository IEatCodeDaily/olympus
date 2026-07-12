import { describe, expect, it } from "vitest";
import {
  collectVaultSuggestions,
  findVaultSuggestion,
  fromRichMarkdown,
  hasJjConflictMarkers,
  joinVaultMarkdown,
  serializeVaultSuggestion,
  splitVaultMarkdown,
  toRichMarkdown,
  type VaultSuggestion,
} from "./vaultMarkdown";

const preservedFixtures = [
  "<!-- keep this exact -->",
  "Footnote[^1]\n\n[^1]: detail",
  "Read [the design][design].\n\n[design]: /design.md \"Design\"",
  ":::warning {#careful}\nDo not normalize me.\n:::",
  "<Component value={\"raw\"} />",
  "import X from \"./x\"",
  "export default function Foo() {}",
] as const;

describe("Vault Markdown frontmatter boundary", () => {
  it("splits and rejoins YAML frontmatter byte-for-byte", () => {
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
  it("shows preserved syntax literally in the rich surface", () => {
    expect(toRichMarkdown("<!-- keep this exact -->")).toContain("<!-- keep this exact -->");
  });

  it.each(preservedFixtures)("round-trips preserved rich-editor syntax: %s", (markdown) => {
    expect(fromRichMarkdown(toRichMarkdown(markdown))).toBe(markdown);
  });

  it.each([
    ["HTML comment", "```html\n<!-- literal -->\n```"],
    ["MDX import", "```mdx\nimport X from \"./x\"\n```"],
    ["MDX tag", "```mdx\n<Component value={\"raw\"} />\n```"],
    [
      "preservation marker",
      "```olympus-preserved:not-encoded\nliteral user-authored content\n```",
    ],
  ])("leaves user-authored fenced %s byte-identical", (_name, markdown) => {
    expect(toRichMarkdown(markdown)).toBe(markdown);
    expect(fromRichMarkdown(markdown)).toBe(markdown);
  });

  it("bridges wikilinks only in prose and decodes malformed links defensively", () => {
    const markdown = [
      "See [[docs/design.md|Design]].",
      "",
      "`[[inline.md]]`",
      "",
      "````md",
      "```",
      "[[nested-fence.md]]",
      "```",
      "````",
    ].join("\n");
    const rich = toRichMarkdown(markdown);
    expect(rich).toContain("olympus-wikilink:");
    expect(rich).toContain("`[[inline.md]]`");
    expect(rich).toContain("[[nested-fence.md]]");
    expect(fromRichMarkdown(rich)).toBe(markdown);
    expect(fromRichMarkdown("[bad](olympus-wikilink:%E0%A4%A)")).toBe("[bad](olympus-wikilink:%E0%A4%A)");
  });

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
