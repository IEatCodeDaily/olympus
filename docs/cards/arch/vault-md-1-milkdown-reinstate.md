# VAULT-MD-1 · Reinstate Milkdown as the Vault's default WYSIWYG editor — properly

## Goal
Operator decision 2026-07-12 (reverses postmortem 0018's resolution, not its
lessons): the Vault note editor becomes **Milkdown/Crepe WYSIWYG by default**.
The v1 failure was NOT Milkdown itself — it was the forced-ejection design (a
regex denylist that kicked users to source mode for ordinary Markdown). v2
inverts it: **unsupported syntax is preserved as bytes and the editor STAYS
rich.** No automatic mode switching, ever.

## Read FIRST (all — this feature failed once already)
- `docs/design/vault-milkdown-integration-notes.md` — the pitfall map from the
  v1 cycle: reversible wikilink adapters (code-range-aware), frontmatter
  byte-preservation, the React StrictMode create/destroy race (serialize via
  promise ref), sync serialization after slash-menu commands, lazy loading.
  Every one of these was learned the hard way. Follow them.
- `docs/postmortems/0018-vault-editor-workbench-qa-gap.md` — why v1 died. Your
  QA must not repeat it: NEVER mock the real editor in tests; browser journeys
  must enter edit mode and type representative Markdown.
- `git show 1477c41` — the v1 Milkdown implementation (deleted at 43ec4a1).
  Salvage the adapters and suggestion providers; do NOT salvage the
  Rich/Source toggle or `richEditorFallbackReason` denylist.
- Current editor: `ui/src/views/vaults/editor/` (CodeMirror live-preview) —
  it STAYS, demoted to the source/conflict surface.
- `ui/src/views/vaults/components/VaultWorkspace.tsx` — tabs/panes; keep the
  VS Code model + the dirty-`*` tab title exactly as-is.

## The design (settled — do not re-litigate)
1. **Milkdown/Crepe is the default surface** for every non-conflicted note.
2. **No mode toggle buttons.** An explicit "Edit source" action lives in the
   note's overflow/kebab menu (and Cmd/Ctrl+Shift+E), switching that tab to
   the CodeMirror surface; "Edit rich" switches back. User-initiated only.
3. **No denylist, no forced fallback.** Constructs Milkdown can't model
   (comments, footnotes, reference defs, directives, MDX-ish tags) are
   preserved via passthrough/raw nodes that render as literal text blocks in
   the rich surface and re-serialize byte-identically. If a construct truly
   cannot be preserved through a round-trip, that is a FAILING TEST, not a
   reason to eject the user.
4. **The one exception: unresolved jj conflict markers.** A conflicted note
   opens in the CodeMirror surface with the existing warning banner. This is
   the ONLY automatic source-mode path.
5. **Normalization policy (operator-accepted):** opening a note NEVER rewrites
   it (init/normalization must not trip dirty state — compare against loaded
   bytes). Saving after rich edits MAY normalize formatting (whitespace,
   marker style); content, frontmatter, wikilinks, and preserved constructs
   must survive byte-exact. Document this in the design doc.
6. **Frontmatter is byte-preserved outside the editor** — rich-edit the body
   only, re-join untouched frontmatter on emit (v1 pattern, keep it).
7. **Wikilinks/mentions/labels**: reversible bridge adapters from v1, with the
   code-range awareness fix (never transform inside code spans/fences) and
   malformed-percent-encoding defense. Suggestions via Milkdown slash/mention
   menus with the synchronous-serialize-after-command fix.
8. Dirty state = serialized-current ≠ loaded bytes; tab shows `*` (existing).
   Explicit Save/Cancel stays.

## QA gates (the postmortem's teeth)
- Component tests mount the REAL Milkdown editor (jsdom shims as needed) — a
  mocked editor is an automatic review reject.
- Round-trip fixture suite: for each of {wikilinks, frontmatter, comments,
  footnotes, reference links, directives, MDX-ish tags, nested code fences
  containing wikilink-shaped literals, unicode}: load → mount rich → no dirty
  → simulate trivial edit + undo or serialize → assert content-preserving
  output per policy §5 (byte-exact for preserved constructs + frontmatter).
- StrictMode test: exactly one `.ProseMirror` after mount (the v1 race).
- Maestro desktop + mobile flows: enter a note → rich editor visible → type →
  bold/heading via toolbar or markdown input rules → save → reload → content
  correct; open a conflicted fixture note → CodeMirror + warning shown;
  kebab → Edit source → CodeMirror → Edit rich → back, no data loss.
  Screenshots captured; evidence bundle per ui/scripts/evidence-bundle.sh.
- `npm run typecheck` + `vitest --run` + production build + react-doctor on
  changed scope. Lazy-load Milkdown chunks (v1 did; keep it — watch the
  bundle advisory).
- Update `docs/design/vault-milkdown-integration-notes.md` with anything new
  you learn, and add a DESIGN_SYSTEM.md changelog entry noting the operator
  reversal of 0018's resolution (lessons retained, ejection design replaced).

## Environment notes
- Global NODE_ENV=production on this host — `env -u NODE_ENV` or explicit
  development for installs/tests, or devDeps silently prune.
- /tmp is a 1G tmpfs: TMPDIR=/home/rpw/.cache/tmp, and for Maestro also
  JAVA_TOOL_OPTIONS=-Djava.io.tmpdir=/home/rpw/.cache/tmp.
- Vite mock env for Maestro: NODE_ENV=development VITE_USE_MOCKS=true
  VITE_API_BASE=http://127.0.0.1:8787; beware stale Vite children holding the
  port with baked-in wrong env — verify the listening PID.
- The box is compile-contended; UI work is fine but don't run cargo.

## Gates
- Do not push to main. Green + evidence → `blocked: review-required` with
  fixture results and screenshot paths.
