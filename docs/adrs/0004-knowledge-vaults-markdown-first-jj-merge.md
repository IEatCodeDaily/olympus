# ADR 0004: Knowledge vaults are markdown-first + jj merge, not CRDT

- Status: Accepted
- Date: 2026-06-29
- Relates to: ADR 0002 (§8 vaults), ADR 0003 (substrate — vaults are a
  separate subsystem with the **opposite** writer model), Epic K (vaults),
  Epic P (local-first content plane — partially resolved by this ADR)

## Context

Epic K specifies knowledge vaults as "text = jj, binaries = blobref +
content-addressed." Epic P reserved a "CRDT content plane (iroh-docs)" as
research. Between those two notes sat an open question: **what is the vault
document format, and what merge engine guarantees correctness under
multi-writer local-first sync?** This ADR resolves that question.

Two properties of the vault shape the decision:

1. **Multi-writer, local-first.** Unlike the control plane (ADR 0003: single
   authority, single-writer serialization), vaults are edited concurrently by
   multiple agents and humans across nodes. A node may edit while disconnected
   and sync later. This is the opposite writer model from the control plane,
   and the correctness mechanism must be different.

2. **AI-native.** Every vault document must be readable and writable by AI
   agents as a first-class operation, not a second-class path. The format the
   AI sees must be the format the human sees — no opaque intermediate.

A design conversation evaluated the obvious candidate for multi-writer
document merge — **CRDTs** — across three layers.

### Layer 1: structured data (sheets, databases, tables)

Structured vault data needs real multi-writer merge with per-row / per-column
granularity. The only SQLite-shaped engine that fits on-demand selective sync
is **cr-sqlite** (merge-on-connect, per-site vector clock, LWW per column).
Litestream (continuous WAL, single-writer, always-on), dqlite/rqlite (Raft
quorum, not selective), and raw `.db` file sync (corrupts SQLite — two nodes
opening the same `.db` over a file-sync layer produces torn WAL, lost
transactions) were all rejected as wrong-shaped. Postgres-backed data is
single-writer on the main server, accessed by nodes via iroh RPC — no merge
needed, no cr-sqlite.

This layer is decided: **cr-sqlite for SQLite vault DBs; iroh RPC to
single-writer Postgres for system/agent DBs.**

### Layer 2: document content (notes, knowledge prose)

This is where the real decision lives. The conversation evaluated, in order:

1. **CRDT block editors (BlockSuite, BlockNote, raw Tiptap+Yjs).** These give
   true CRDT merge on rich block documents but: (a) store JSON, not plaintext
   — kills grep/diff/terminal readability and agent `cat`; (b) require `yrs`
   (Rust Yjs) in the backend to extract text for indexing/embedding, because
   the editor's doc is opaque to headless nodes; (c) tempt you to embed
   structured data (sheets, databases) inside the CRDT doc, creating a
   dual-source problem where the embed's snapshot can diverge from the backing
   store; (d) the richest editor (BlockSuite) has database/whiteboard block
   types with **no markdown representation**, breaking AI-nativeness for any
   doc that uses them — no model has training exposure to BlockSuite's schema,
   so AI generation of valid block trees is unreliable.

2. **View-only embeds + external editing.** Resolves the embed problem:
   embeds are view-only references (typed links) to backing stores; edits go
   to the store, never the doc. Sheets are database instances, not doc blocks.
   This removes in-doc CRDT complexity for structured data entirely — but the
   prose itself still needs a merge engine.

3. **Markdown-first.** The pivot: native markdown + YAML frontmatter as the
   source of truth, a WYSIWYG editor that serializes to `.md` on save, and a
   text merge engine for concurrent edits. This makes the AI surface native
   (the AI reads/writes the same format humans do), keeps files greppable and
   diffable, and removes the `yrs` backend dependency entirely. The merge
   engine is **jj** (Jujutsu) — a Git-compatible, Rust-native VCS with
   structured merge that writes conflict markers into the working copy for
   human/agent resolution.

### Layer 3: the merge engine

jj was chosen over hand-rolled text 3-way merge or mdast-based AST merge
because: it's already in the roadmap (Epic E), it's Rust-native, it's
Git-compatible (vaults interop with git tooling), its conflict model is
superior to git's (conflicts are first-class objects resolved in the working
copy, not blocked at merge time), and it means **zero custom merge code.**
The vault is a jj repo per node; sync is exchanging jj/git state over iroh.

## Decision

**Vault documents are native markdown + YAML frontmatter.** No CRDT, no JSON
block store, no intermediate editor model persisted to disk. The `.md` file
is the single source of truth.

**Jujutsu (jj) is the merge engine** for all vault text content, including
frontmatter. Each vault is a jj repo; multi-node sync exchanges jj/git state
over iroh. Concurrent edits to different paragraphs merge cleanly; concurrent
edits to the same paragraph collide and surface as inline conflict markers
for human or agent resolution.

**A WYSIWYG editor renders markdown richly** (headings, lists, tables, code
blocks, embeds) but serializes to `.md` on save. The editor's in-memory model
is ephemeral — never the source of truth. Block structure is inferred from
markdown at render time (headings, blank-line-separated paragraphs, lists),
not stored as persistent block IDs.

**cr-sqlite backs structured vault data** (databases, sheets-as-databases,
any tabular data). SQLite vault DBs use cr-sqlite for multi-writer
merge-on-connect. Postgres DBs are single-writer on the main server, accessed
via iroh RPC. Both are referenced from docs as view-only embeds via a link
file (engine + location descriptor).

**Embeds are view-only references, never data containers.** A doc embeds a
database or sheet as a typed link (e.g. `[db: active notes](vault://notes.db?table=notes&filter=...)`),
rendered live by the editor. Edits to embedded data go to the backing store
(cr-sqlite / iroh RPC), never to the doc. The doc never holds data snapshots.
This eliminates the dual-source problem entirely.

**AI agents read and write native markdown.** No bridge layer, no format
translation. The AI emits markdown; the file is the format. Structured-data
edits go through the storage API (cr-sqlite / iroh RPC), not the doc.

### The writer-model split (explicit, load-bearing)

| Subsystem | Writer model | Merge engine | Source of truth |
|---|---|---|---|
| Control plane (ADR 0002/0003) | Single-writer | redb event log (serialized) | redb append-only log |
| Vault structured data (tables) | Multi-writer | cr-sqlite (LWW per column) | cr-sqlite tables |
| Vault documents (prose) | Multi-writer | jj (text 3-way merge) | `.md` files |
| System/agent data (Postgres) | Single-writer | MVCC (single server) | Postgres on main |

These are four different correctness mechanisms for four different access
patterns. This is intentional, not accidental complexity — the control plane
needs audit-grade serialization; vault prose needs human-friendly merge;
structured vault data needs per-cell convergence; system data needs a central
authority. Forcing one mechanism across all four would be wrong.

### Sync model

- **Documents (`.md` + frontmatter):** jj repo per vault, synced on demand by
  exchanging git/jj state over iroh. Selective folder sync is the default; a
  node syncs only the vault paths it needs.
- **Structured data (SQLite):** cr-sqlite merge-on-connect. The `.db` file is
  a local artifact — never transported as a blind file copy. The link file in
  the vault holds metadata (tables, site ID, last merge clock), not the DB.
- **Structured data (Postgres):** no sync — nodes call the main server via
  iroh RPC. The link file holds the iroh node ID + RPC endpoint, not
  credentials (creds live in per-node `~/.olympus/secrets`, 0600).
- **Binaries (images, attachments):** content-addressed (iroh-blobs + R2/S3
  backup), pulled by hash on demand.
- **Index, embeddings, tree:** derived locally per node from synced content.
  Never synced as data. Computed from `.md` files + structured-data link
  targets. Two nodes computing the same doc's embedding produce identical
  vectors (content-addressed) — no merge needed, just dedupe by hash.

## Consequences

- **Gained:** AI-nativeness is free — markdown is the format, no bridge layer,
  no format translation, no opaque intermediate. The AI reads and writes the
  exact same bytes a human sees in a terminal.
- **Gained:** vault files are greppable, diffable, readable in terminals and
  SSH sessions. Agents can `cat` a note. `git diff` (via jj's git compat)
  shows meaningful prose changes, not walls of JSON.
- **Gained:** zero custom merge code. jj owns text merge; cr-sqlite owns
  structured merge. We write no diff3, no AST walker, no conflict resolver.
- **Gained:** `yrs` is not a dependency. No Rust Yjs backend, no opaque doc
  state, no text-extraction layer for indexing. The backend reads `.md`
  directly.
- **Gained:** editor choice is decoupled from storage. Any markdown WYSIWYG
  (Milkdown, TipTap+md, Lexical+md) works; switching editors does not migrate
  data because the format is markdown, not an editor-specific block schema.
- **Cost (owned):** concurrent same-paragraph edits collide and require
  resolution (inline conflict markers, resolved by human or agent). This is
  the fundamental correctness concession of text 3-way merge over CRDT. It is
  acceptable for knowledge prose — paragraph-level contention is rare, and
  Git has run on this model for decades. It would NOT be acceptable for
  high-frequency collaborative editing of a single paragraph, but vaults are
  not that workload.
- **Cost:** frontmatter merges at line level (jj treats it as text). Different
  keys merge cleanly; same key on concurrent edits collides. Acceptable —
  frontmatter is small and key-level contention is rare.
- **Cost:** block structure has no persistent identity. "Paragraph 3 moved
  below paragraph 5" is indistinguishable from "paragraph 3 deleted, new
  paragraph inserted" at the merge level. This means reordering combined with
  concurrent edits to the moved region can collide. Acceptable for prose;
  would not be for a structured canvas.
- **Embed limitation:** embeds are view-only. If a future requirement demands
  in-place editing of structured data inside a doc (a human typing into a
  spreadsheet cell rendered in the doc), this architecture does not support
  it without delegating the edit to the backing store's own full-screen edit
  surface. The planned answer: sheets/databases have their own editor route;
  the doc embed is a live view, not an edit surface.
- **Resolves Epic P (partially):** the vault document layer does NOT use a
  CRDT. Epic P's "CRDT content plane (iroh-docs)" research question is
  answered for docs/notes: **markdown + jj, not CRDT.** The P2P message sync
  layer and mobile companion remain research under Epic P.

## Rejected alternatives

- **BlockSuite (block-CRDT editor).** Most complete rich editor with native
  block-CRDT (built on Yjs), but its database/whiteboard block types have no
  markdown representation — breaks AI-nativeness for any doc using them. No
  LLM has meaningful training exposure to BlockSuite's schema; AI generation
  of valid block trees is unreliable. JSON storage kills terminal/grep/diff.
  Rejected.
- **BlockNote (Notion-like, Yjs-backed via y-prosemirror).** Good for
  view-only embeds and simple blocks; ceiling reached on structured embeds
  (recursive nesting, inline-editable regions). Migration to raw Tiptap when
  the ceiling is hit is data-trivial but throws away all editor-layer code.
  Once embeds are view-only and sheets are separate surfaces, BlockNote's
  remaining advantage (Notion-like block UX) doesn't justify the JSON store
  + `yrs` backend cost. Rejected in favor of markdown-first.
- **Raw Tiptap + Yjs + `yrs` backend.** Full control, no ceiling, but:
  requires `yrs` in Rust to extract text for indexing (editor doc is opaque to
  headless nodes); JSON storage; the AI needs a markdown bridge anyway.
  Strictly more complexity than the markdown-first path for no gain once
  embeds are view-only and sheets are separate surfaces.
- **Hand-rolled text 3-way merge.** Same merge semantics as jj, but we write
  and maintain it. Rejected — jj already exists, is Rust-native, is in the
  roadmap (Epic E), and has a superior conflict model to anything we'd build.
- **mdast + injected block IDs + AST 3-way merge.** Better merge than text
  3-way (handles block reordering via stable IDs in HTML comments), keeps
  `.md` on disk. Rejected for now — the complexity is not justified until
  paragraph-level contention is observed in practice. jj's text merge is the
  starting point; **AST merge is the documented upgrade path** if merge
  quality becomes a real complaint. The upgrade is non-disruptive: IDs are
  injected as comments, invisible to readers and the AI surface.
- **Raw `.db` file sync for SQLite vaults.** Two nodes opening the same `.db`
  over a file-sync layer produces torn WAL, lost transactions, corrupted
  `-shm`/`-wal` siblings. This is the most reliable way to brick a SQLite
  store. Rejected unconditionally — cr-sqlite's merge protocol is the
  transport, never file copy.
