# Vault Feature Implementation Plan

> **Status:** Research complete. Executing now.

## Research findings

### Editor choice (per ADR 0004)
The ADR decouples editor from storage — **any WYSIWYG that serializes to .md works**.
The current textarea editor is the quick-and-dirty baseline. Upgrade path options:

| Editor | Bundle | Pros | Cons | Verdict |
|---|---|---|---|---|
| **Textarea (current)** | 0 | Already works, AI-native, zero deps | No WYSIWYG, no syntax highlighting | Keep as fallback |
| **CodeMirror 6** | ~150kb | Best-in-class syntax highlighting, vim mode, lightweight, used by Obsidian's mobile editor | Source editor only (not WYSIWYG) | **Phase 1 upgrade** — best ROI |
| **Milkdown** | ~200kb | True WYSIWYG, plugin system, serializes to markdown | Heavy, complex setup, React wrapper is finicky | Defer — textarea+CodeMirror is "quick and dirty" per user directive |
| **TipTap** | ~100kb | Rich, Notion-like | Persists JSON by default, must force-markdown | Reject — JSON-block original sin per ADR |

**Decision: CodeMirror 6 for the editor** (syntax highlighting, vim mode, markdown language support). It's what Obsidian's mobile editor uses. It serializes to plain markdown. Lightweight. The user asked for "quick and dirty markdown editor is good enough" — CodeMirror is the right step up from a bare textarea without over-engineering.

### Graph view
- **d3-force** is the canonical implementation (Obsidian itself uses a custom force simulation based on d3-force primitives).
- `react-force-graph-2d` wraps d3-force with React bindings and canvas rendering — handles 1000+ nodes smoothly.
- For Olympus vaults (typically <500 notes per vault), a canvas-based d3-force graph is correct.
- **Decision: `react-force-graph-2d`** (canvas, handles zoom/pan/drag natively, React-idiomatic).

### Content addressing (per ADR 0004 §sync model)
The ADR specifies: "binaries are content-addressed (iroh-blobs + R2/S3 backup), pulled by hash on demand."
For **note content** itself: the CID is derived — "Two nodes computing the same doc's embedding produce identical vectors (content-addressed)."
- **Decision: BLAKE3 hash** of the markdown content, stored in frontmatter as `cid: <blake3-hex>`.
  BLAKE3 is faster than SHA-256, tree-structured (can hash large files in parallel), and is what iroh uses internally.
  The Rust `blake3` crate is the canonical impl. This gives us:
  - Dedup: identical notes across vaults have the same CID
  - Integrity: detect corruption/tampering
  - Future iroh-blobs integration: the hash is already the content address

### Database feature (per ADR 0004 §structured data)
"cr-sqlite backs structured vault data." The database/table feature is:
- A SQLite `.db` file per vault, with cr-sqlite for multi-writer merge
- Embedded in notes as view-only references (`[db: notes](vault://notes.db?table=notes)`)
- For the MVP: **CRUD SQLite tables via API**, rendered as a sortable/filterable table in the UI
- **Decision: Start with read-only table view from frontmatter-defined collections** (simpler, no cr-sqlite dep yet).
  A note with `collection: true` in frontmatter defines a data collection; rows are child notes with structured frontmatter fields. This gives us Obsidian Dataview-like functionality without a database engine.

---

## Phase 1: Backend — content addressing + graph data + collections API

### Task 1.1: Content hash (BLAKE3) on note write
**File:** `crates/control-plane/Cargo.toml` (add `blake3 = "1"`), `crates/control-plane/src/vault.rs`

On `write_note`, compute `blake3::hash(markdown.as_bytes())` and inject `cid: <hex>` into the frontmatter before writing to disk. The `read_note` path already parses frontmatter — expose `cid` in the `NoteDocument` struct.

### Task 1.2: Graph data endpoint
**File:** `crates/control-plane/src/vault.rs` (new method), `crates/control-plane/src/server/mod.rs` (route)

`GET /api/vaults/:id/graph` → `{ nodes: [{ id, title, cid }], edges: [{ source, target }] }`
Builds from `list_notes` + `linked_notes` on each note. O(n*m) where n=notes, m=avg links — fine for <500 notes.

### Task 1.3: Collection/table endpoint
**File:** `crates/control-plane/src/vault.rs`

`GET /api/vaults/:id/collections` → scans all notes for `collection: true` in frontmatter.
`GET /api/vaults/:id/collections/:name/rows` → for a collection defined by note X, returns all child notes in the same folder with their frontmatter fields as rows.

---

## Phase 2: UI — CodeMirror editor + graph view + collections table

### Task 2.1: CodeMirror editor
**Files:** `ui/package.json` (add deps), `ui/src/views/vaults/pages/NotePage.tsx`

Replace the textarea with CodeMirror 6 (`@uiw/react-codemirror`, `@codemirror/lang-markdown`, `@codemirror/theme-one-dark`).
- Split-pane: editor | preview (toggle or side-by-side on desktop, tabbed on mobile)
- Syntax highlighting for markdown
- Serializes to plain .md on save (already wired via `putVaultNote`)

### Task 2.2: Force-directed graph view
**Files:** `ui/package.json` (add `react-force-graph-2d`), `ui/src/views/vaults/pages/GraphPage.tsx`

Replace the stub with a real interactive graph:
- Fetch `GET /api/vaults/:id/graph`
- Nodes = notes (sized by link count, colored by folder)
- Edges = wikilinks
- Click a node → navigate to that note
- Zoom/pan/drag (built into react-force-graph)

### Task 2.3: Collections/table view
**Files:** `ui/src/views/vaults/pages/TablesPage.tsx`

Replace the stub with a data table:
- Fetch `GET /api/vaults/:id/collections`
- Select a collection → render rows as a sortable table
- Columns derived from frontmatter fields
- Click a row → open the underlying note

### Task 2.4: Wikilink autocomplete
**Files:** `ui/src/views/vaults/components/WikilinkAutocomplete.tsx`

In the CodeMirror editor, typing `[[` triggers an autocomplete dropdown of note titles. Selecting inserts `[[Note Title]]`. On save, the backend's `parse_linked_notes` resolves the link.

---

## Phase 3: Tests + evidence

### Task 3.1: Rust tests for new vault methods
- Content hash injection + retrieval
- Graph data structure correctness
- Collection scanning

### Task 3.2: E2e tests
- `vaults.spec.ts`: create vault → create note → edit with CodeMirror → save → verify content → graph view renders nodes/edges → collections table renders
