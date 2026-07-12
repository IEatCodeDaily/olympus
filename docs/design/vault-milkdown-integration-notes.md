# Vault rich Markdown editor integration

Use this when adding a structured/rich editor to an Olympus Vault while Markdown remains canonical.

## Authority boundary

- Persist only Markdown plus YAML frontmatter. Editor trees (ProseMirror JSON), collaboration state, and widget state are transient.
- Keep explicit save/cancel and compute dirty state against the originally loaded Markdown. Editor initialization or normalization must not call the page-level change callback.
- Keep CodeMirror as the exact-source surface for source editing, jj conflicts, malformed frontmatter, and syntax the rich parser cannot safely round-trip.
- Fail closed: detect unsupported constructs before mounting the rich editor. At minimum account for unresolved conflict markers, incomplete frontmatter, raw HTML/MDX, directives, and footnotes unless a tested extension handles them.
- Preserve frontmatter byte-for-byte outside the rich editor; rich-edit only the body and join the untouched frontmatter when emitting changes.

## Reversible syntax adapters

Milkdown parses Markdown into ProseMirror and serializes it again, so unknown syntax may be escaped or normalized.

- Convert Olympus-only syntax such as `[[wikilinks]]` into an ordinary, typed temporary Markdown link before parsing, e.g. an `olympus-wikilink:` URL containing the encoded original source.
- Convert that temporary representation back to the exact canonical syntax after serialization.
- Apply both directions only to prose. A global regex silently rewrites wikilink-shaped literals inside inline code, fenced code, and indented code blocks; walk Markdown lines/code spans and leave those ranges byte-identical.
- Decode internal bridge URLs defensively: malformed percent encoding must preserve the original link rather than throwing, and decoded values must match the expected canonical `[[...]]` shape before replacement.
- Also handle the escaped form Milkdown produces while a user types a new wikilink (`\\[\\[target]]` or escaped closing brackets), again only outside code.
- Test pre-existing syntax, newly typed/inserted syntax, code-literal non-transformation, and malformed bridge URLs. Use a real browser for editor round-tripping; parser utility tests alone do not prove the integration.
- Typed links such as `vault://...` remain references to Hall-owned projections. Never copy referenced SQLite rows into editor state.

## Suggestions and command insertion

- Slash, mention, label, and wikilink menus are transient UI. Their providers should be pluggable and eventually Hall-backed.
- Derive the active trigger from the current ProseMirror selection's parent text, not from the entire serialized document; otherwise an old trigger elsewhere can reopen the menu.
- Compute replacement positions from the current ProseMirror selection. Preserve focus after insertion.
- For syntax Milkdown would escape as plain text, insert into ProseMirror and immediately run the canonical serializer adapter.
- Do not rely solely on Milkdown's asynchronous `markdownUpdated` callback after a menu command. Emit the freshly serialized canonical Markdown synchronously after dispatching the command, or an immediate Rich→Source switch can show/save the pre-command draft.
- Provide mouse and keyboard handling (Escape, arrows, Enter), with `listbox`/`option` semantics and `aria-selected`.

## React lifecycle pitfall

Crepe creation/destruction is asynchronous. React Strict Mode replays effects, so naïve `create()`/cleanup `destroy()` can leave duplicate ProseMirror editors or let an old destroy clear the new editor.

Serialize lifecycle operations through a promise held in a ref:

1. Chain `create()` after the previous teardown promise.
2. Set the active editor ref only after creation completes and the effect is still live.
3. In cleanup, chain `destroy()` after that creation promise and store the teardown promise for the replayed effect.
4. Ignore listener callbacks after disposal.

Prove this with an E2E assertion that exactly one `.ProseMirror` exists after entering edit mode.

## Loading and verification

- Lazy-load the rich editor from the note page. Crepe/Milkdown, CodeMirror language support, Vue internals, and KaTeX are large; they should not inflate every Olympus route's initial bundle.
- Required checks: focused adapter tests, full UI tests, typecheck, production build, React Doctor, and Playwright against MSW.
- E2E must prove: mount stays clean, slash menu opens, suggestions insert canonical text, wikilinks survive Rich→Source, explicit save persists, and unsafe documents start in source mode.
- If Playwright suddenly times out across unrelated routes, check host pressure before changing tests. Concurrent Rust linking can exhaust RAM/swap and delay Vite dynamic chunks; rerun after the competing build finishes rather than weakening assertions.
