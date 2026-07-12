# Vault rich Markdown editor integration

Use this when adding a structured/rich editor to an Olympus Vault while Markdown remains canonical.

## Authority boundary

- Persist only Markdown plus YAML frontmatter. Editor trees (ProseMirror JSON), collaboration state, and widget state are transient.
- Keep explicit save/cancel and compute dirty state against the originally loaded Markdown. Editor initialization or normalization must not call the page-level change callback.
- Milkdown/Crepe is the default surface for every non-conflicted note. CodeMirror remains available only through the operator-invoked **Edit source** action (or Cmd/Ctrl+Shift+E), plus the automatic unresolved-jj-conflict path. Never switch modes because of ordinary Markdown syntax.
- Preserve unsupported constructs through reversible `olympus-preserved` passthrough blocks. They render literally in rich mode and decode back to their original bytes; comments, footnotes, reference definitions, directives, MDX-ish tags, and import/export paragraphs are covered by real-editor fixtures. A failed round trip is a test failure, not a fallback reason.
- Preserve frontmatter byte-for-byte outside the rich editor; rich-edit only the body and join the untouched frontmatter when emitting changes.
- Opening/mounting must not emit Milkdown's normalized serialization. Dirty state compares emitted canonical Markdown with the loaded bytes. Once the operator makes a rich edit, Milkdown may normalize ordinary Markdown formatting, while frontmatter, wikilinks, and passthrough constructs remain byte-identical.

## Accepted integration contract

This contract supersedes the first Milkdown integration from `1477c41` and the postmortem 0018 temporary resolution. Postmortem 0018 correctly identified the QA gap and the lossy denylist failure mode, but the accepted product direction is to reinstate Milkdown/Crepe as the primary editor without source ejection.

### Component boundaries

- `NotePage` owns the originally loaded note bytes. On note load it stores both `loadedMarkdown` and `draftMarkdown`; dirty state is always `draftMarkdown !== loadedMarkdown` until a successful save refreshes both from the server response/query invalidation. Do not compare against Milkdown's initial serialization.
- `VaultMarkdownEditor` owns editor mode and top-level note actions. Its default mode is `rich` unless `hasJjConflictMarkers(markdown)` is true. It renders `MilkdownRichEditor` for rich mode and `SourceMarkdownEditor` for explicit source mode or unresolved jj conflicts, with the same Save/Cancel/Delete actions independent of mode.
- `MilkdownRichEditor` owns Crepe creation, ProseMirror command insertion, and suggestion popovers. It receives the complete Markdown document, splits frontmatter/body internally, sends complete canonical Markdown to `onChange`, and never exposes ProseMirror JSON or editor-local state upward.
- `vaultMarkdown.ts` owns pure string adapters only: frontmatter splitting, wikilink bridge encoding/decoding, raw-node preservation, suggestion parsing, and conflict detection. These helpers must stay deterministic and covered by fixtures; they must not inspect DOM state or editor instances.

### Loading and editor selection

- Lazy-load the note editor from `NotePage`, and lazy-load `MilkdownRichEditor` from `VaultMarkdownEditor`. Crepe/Milkdown, CodeMirror, Vue internals, and KaTeX must not enter the initial Olympus route bundle.
- The only automatic source-mode path is a complete unresolved jj conflict marker set: `<<<<<<<`, `=======`, and `>>>>>>>`. A conflicted note starts in source mode and shows the unresolved-conflict warning.
- Source mode for non-conflicted notes is user-only: the overflow **Edit source** action or Cmd/Ctrl+Shift+E. Returning to rich mode is also user-only through **Edit rich** or the same shortcut.
- There is no always-visible Rich/Source segmented toggle. The editor may expose an overflow action labelled **Edit source** / **Edit rich**, but ordinary users should see Milkdown as the normal note surface.

### Explicit prohibitions

- Do not restore the v1 regex denylist (`richEditorFallbackReason`) that treated malformed frontmatter, raw HTML/MDX, comments, directives, footnotes, reference definitions, or import/export paragraphs as automatic source-only reasons.
- Do not add an automatic fallback from rich to source when a round trip fails. That is a bug: add/repair a preservation adapter and a fixture.
- Do not make editor choice depend on ordinary Markdown syntax. Tables, comments, footnotes, reference links, directives, MDX-ish tags, wikilinks, and frontmatter are not fallback triggers.
- Do not persist editor trees, Milkdown plugin state, selected suggestion state, or source/rich mode preference as note content.

## Preservation and normalization policy

Opening a note is read-only from the user's point of view: mounting Milkdown, running adapter transforms, or receiving Crepe's initial normalized Markdown must not call `onChange`, set dirty, rewrite the draft, or enable Save.

After the operator makes a rich edit, the saved Markdown may normalize ordinary Markdown formatting that Milkdown owns, such as list marker spacing, emphasis delimiter choice, heading spacing, or wrapping around edited prose. Required byte fidelity remains stricter for these regions:

- Recognized YAML frontmatter, including fence style, line endings, comments, key ordering, indentation, and YAML content that is syntactically unusual but enclosed by fences. Frontmatter is split before rich editing and joined back unchanged.
- Existing canonical wikilinks, including path, alias, bracket shape, and placement outside code spans/fences. New wikilinks inserted by suggestions use the canonical `[[path|label]]` form.
- Unsupported preserved constructs encoded as `olympus-preserved` passthrough blocks. The decoded bytes must exactly match the original construct, including whitespace and line endings.
- Code spans, fenced code blocks, and indented code blocks. Adapter passes must not rewrite wikilink-shaped text or preserved-looking syntax inside code.

Dirty comparison is against the originally loaded complete Markdown bytes, not against the split body, adapter input, or Milkdown's default serialization. A note that is opened and then saved without an operator edit should be impossible because Save remains disabled.

## Frontmatter, body, and raw-node handling

- `splitVaultMarkdown(markdown)` recognizes only frontmatter that starts at byte zero with `---\n` or `---\r\n` and has a matching closing `---` fence. If no matching closing fence exists, treat the entire document as body so the user can repair it in rich mode unless jj conflict markers force source mode.
- `MilkdownRichEditor` passes only `body` through `toRichMarkdown` and Crepe. `frontmatter` is stored in a ref created from the loaded complete Markdown and is rejoined with `joinVaultMarkdown` for every emitted change.
- Raw-node preservation is a reversible bridge, not a warning. Before parsing, unsupported paragraphs are wrapped in fenced blocks with an `olympus-preserved:<encoded-original>` info string and literal visible content. After serialization, the encoded original replaces the whole preserved fence.
- Preservation applies at paragraph/block granularity for comments, footnote definitions/usages, reference link definitions, directives/containers, MDX-ish JSX tags, and import/export paragraphs. Expand the set by adding new fixture rows; never by broadening source-mode fallback.
- Preservation detection must track Markdown fence boundaries before classifying paragraphs. Fenced comments, imports, JSX, and even literal `olympus-preserved:` examples are user-authored code and must remain byte-identical rather than being wrapped or decoded.
- Bridge decoding is defensive. Malformed percent encoding, or an encoded payload that does not exactly equal the visible passthrough body, leaves the visible Markdown untouched instead of throwing or dropping content.

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

The note page must also avoid mounting a newly keyed editor with the previous tab's draft during a note switch. Wait until the fetched note path and initialized draft path agree before mounting Milkdown; otherwise its one-time `defaultValue` captures stale bytes and the selected tab can display the prior note indefinitely.

## Fixture matrix

Each row needs a pure adapter test and, where marked, a real-editor component or browser assertion. The expected result for every non-conflict row is: starts in rich mode, opening is clean, Rich→Source is user-invoked only, and Save becomes enabled only after a user edit.

| Fixture | Example | Required assertions |
| --- | --- | --- |
| Plain Markdown | `# Title\n\nBody` | Rich default; one `.ProseMirror`; mount emits no change; rich edit can save normalized ordinary Markdown. |
| Frontmatter + body | `---\ntitle: Vault\n---\n# Body` | Frontmatter is not in the editable ProseMirror body; emitted Markdown rejoins byte-identical frontmatter. |
| CRLF frontmatter | `---\r\ntitle: Vault\r\n---\r\n# Body\r\n` | Frontmatter line endings and bytes survive rich edit/save unchanged. |
| Malformed frontmatter | `---\ntitle: missing close\n# Body` | No automatic fallback; whole note remains editable unless jj conflict markers are present; opening stays clean. |
| Wikilink prose | `See [[docs/design.md\|Design]].` | `toRichMarkdown` bridges to `olympus-wikilink:`; `fromRichMarkdown` restores exact source. |
| Wikilink in code | `` `[[literal.md]]` `` and fenced code | Adapter leaves code bytes unchanged; no suggestion trigger inside inline code. |
| Newly typed wikilink | Milkdown escaped `\\[\\[target\\]\\]` | Canonical serializer emits `[[target]]` outside code. |
| HTML comment | `<!-- keep this exact -->` | Encoded as `olympus-preserved`; real editor round trip decodes exact bytes. |
| Footnote | `Footnote[^1]\n\n[^1]: detail` | Preserved block round-trips exact bytes through real editor. |
| Reference definition | `Read [x][id].\n\n[id]: /x` | Preserved block round-trips exact bytes through real editor. |
| Directive/container | `:::warning\nBody\n:::` | Preserved block round-trips exact bytes through real editor. |
| MDX-ish JSX | `<Component value={"raw"} />` | Preserved block round-trips exact bytes through real editor. |
| Import/export paragraph | `import X from "./x"` | Preserved block round-trips exact bytes through real editor. |
| Malformed bridge URL | `[bad](olympus-wikilink:%E0%A4%A)` | Decode failure preserves visible link and does not throw. |
| Unresolved jj conflict | Complete `<<<<<<<`/`=======`/`>>>>>>>` marker set | Sole automatic source-mode exception; warning visible; rich mode disabled until markers are resolved. |

## Browser acceptance scenarios

Run these against MSW fixtures in Chromium/Maestro or Playwright with React StrictMode enabled. Component tests may support them, but do not replace the browser pass because the original failure mocked the real editor.

1. **Clean rich mount:** open a normal note. Assert Save is disabled, no dirty marker appears on the tab, no source editor is present, exactly one `.ProseMirror` exists after StrictMode settles, and no `onChange`/PUT occurs.
2. **Rich edit save:** type in the rich editor. Assert the tab shows dirty, Save enables, Save sends the complete Markdown document, and after the successful PUT/query refresh the dirty marker clears.
3. **Frontmatter preservation:** open a fixture with YAML frontmatter, perform a body edit in rich mode, switch to source through **Edit source**, and assert the frontmatter prefix is byte-identical while the body contains the edit.
4. **Preserved constructs:** open a fixture containing a comment, footnote, reference definition, directive, MDX-ish tag, and import/export paragraph. Switch Rich→Source without editing and assert Save is still disabled and all constructs are visible/exact. Then make a body edit, save, reload, and assert those constructs remain byte-identical.
5. **Wikilinks and suggestions:** type `[[red` in rich mode, choose a note suggestion by keyboard and by mouse in separate runs, immediately switch to source, and assert the canonical `[[path|label]]` text is present. Assert an old trigger elsewhere in the document does not reopen the menu.
6. **Operator-only source switch:** for a non-conflicted extended Markdown note, assert there is no visible Rich/Source toggle and no automatic warning. Open the overflow menu, choose **Edit source**, edit text, choose **Edit rich**, and assert no data is lost.
7. **Conflict exception:** open `conflicted-note.md`. Assert it starts in CodeMirror source mode, shows the unresolved jj conflict warning, does not mount `.ProseMirror`, and does not offer automatic rich mode until the marker set is removed by the user.
8. **No denylist regression:** open notes containing malformed frontmatter, comments, footnotes, reference definitions, directives, MDX-ish tags, and import/export paragraphs. Assert each non-conflicted note starts in rich mode; any source-mode start other than unresolved jj conflict fails the test.

## Loading and verification

- Lazy-load the rich editor from the note page. Crepe/Milkdown, CodeMirror language support, Vue internals, and KaTeX are large; they should not inflate every Olympus route's initial bundle.
- Required checks: focused adapter tests, full UI tests, typecheck, production build, React Doctor, and Playwright against MSW.
- E2E must prove: mount stays clean, slash menu opens, suggestions insert canonical text, wikilinks and passthrough blocks survive Rich→Source, explicit save persists, operator-only source switching loses no data, and only conflicted documents start in source mode.
- If Playwright suddenly times out across unrelated routes, check host pressure before changing tests. Concurrent Rust linking can exhaust RAM/swap and delay Vite dynamic chunks; rerun after the competing build finishes rather than weakening assertions.
