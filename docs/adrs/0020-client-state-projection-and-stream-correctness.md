# ADR 0020 — Client State Projection and Stream Correctness

- Status: Proposed (v2 — revised after adversarial review, 2026-07-14)
- Date: 2026-07-14
- Supersedes: none
- Amends: the WS section of `docs/api-contract.md`; `server/ws.rs` frame vocabulary
- Depends on: `crates/control-plane/src/log.rs` (append-only event log),
  the existing per-session `message_id` ordering key
- Related: ADR 0017 (runtime attempts/readiness/connection epochs),
  ADR 0018 (OTel/TTL diagnostics), the project/repo review's §4.3
  (command serialization requirement)

> **v2 note.** v1 of this ADR proposed a `(streamEpoch, streamId)`-scoped
> **contiguous per-session `seq`** stamped off the log write and recovered via
> `GET /api/events?since=`. Adversarial review
> (`/home/rpw/.hermes/workspace/reviews/olympus-adr0020-gpt56-review.md`)
> proved that spine unbuildable on the actual substrate and it is withdrawn.
> Verified facts that killed it:
> - The event log is a **single global `AUTOINCREMENT`** (`log.rs:1082`), not
>   per-session and not scoped — so "contiguous per-session seq == global log
>   seq" is a contradiction.
> - `retain_native` (`log.rs:228-260`) `DELETE`s events on boot, leaving
>   **permanent global-seq holes** — a `seq>watermark+1 → catch-up` rule would
>   loop forever on a hole it can never fill.
> - `GET /api/events` is a **global, org-unscoped, delta-less firehose**
>   (`events.rs:40-64`) — using it for browser catch-up leaks cross-org events
>   and cannot serve token deltas (which are never logged — `sessions.rs:1367`
>   etc. broadcast only).
> - There is **no serialized append+apply seam**: `log.append` releases its
>   lock, then callers take the view lock separately (`support.rs:22-31`,
>   `sessions.rs:1532/1541`) — so "stamp seq off the same event application"
>   presumes a rework that does not exist.
>
> v2 keeps the (correct, well-cited) diagnosis and the three cheap, sound fixes,
> orders the transcript by the **existing per-session `message_id`**, and — only
> where a catch-up cursor is genuinely needed — adds a **session-scoped,
> org-checked** endpoint rather than reusing the global firehose.

## 1. Problem

The browser loses and reorders assistant responses:

1. User sends a message.
2. User navigates to another view/session.
3. User returns; the agent's response is **absent**.
4. User sends a second message.
5. The earlier assistant response then appears **above** the newly sent message.

This is a consistency defect. Root causes, all confirmed in source:

- **Completion is signalled before it is durable (primary cause).**
  `sessions.rs:1516` broadcasts `MessageDone` **before** the durable
  `append_assistant_message` at `:1532` and the view apply at `:1541-1543`.
  On `done`, the client invalidates and refetches (`queries.ts:213`); that
  refetch races the not-yet-committed assistant row and returns a transcript
  **missing** the assistant message.
- **Frames are dropped for unsubscribed sessions.** `should_deliver`
  (`ws.rs:281-303`) withholds session-scoped frames from any connection not
  subscribed to that session. Navigating away unsubscribes; the turn's
  completion frames never reach that connection and are never replayed on
  return.
- **No client convergence.** `useMessages` is `staleTime: Infinity`
  (`queries.ts:77`); `message.delta` is ignored (`:208-210`);
  appended/done only invalidate (`:212-221`); `ChatPage` keeps the live turn in
  component-local `streamParts` + optimistic state (`ChatPage.tsx:92,100`) and
  resets it on session change (`:128-130`), clearing `streamParts` on `done`
  (`:355`).

Sequence producing the exact symptom: the assistant message commits while the
client is unsubscribed or after `done` already fired → refetch misses it → it is
absent on return. The next send invalidates the query; the refetch now returns
the earlier assistant message, rendered in server order — which is correct
order by `message_id`, but it lands *after* the optimistic new user message that
has no committed id yet, so it appears "above" the new message.

Server-side corroboration: managed sessions report `message_count = 0` despite
durable `messages` rows, because `SessionView::apply(MessageAppended)`
(`views/session.rs:201-210`) advances only `last_activity` and never increments
the count, while `MessageView.counts` (`views/message.rs:85`) does — two
projections of the same event disagree, and `SessionDto` reads the stale one.

## 2. Non-goals / rejected framings

- **WASM + WebRTC is not a correctness fix.** WASM is a compile target; WebRTC
  a transport. Neither supplies ordering, durability, or reconciliation. Keep
  WebSocket. WebRTC stays a *future* envoy→client transport optimization only.
- **No second permanent event/log store.** Fixes ride the existing `log.rs`
  and the existing per-session `message_id`.
- **No global-seq / streamEpoch / per-session-contiguous-seq envelope**
  (withdrawn — see v2 note). The transcript already has a correct per-session
  ordering key: `message_id` (per-session `MAX+1`, `log.rs:305-313`; read back
  `ORDER BY message_id`, `log.rs:298`).

## 3. Current state (as built)

```
Hall event log (log.rs)  ── global AUTOINCREMENT seq; retain_native() deletes rows (holes)
  │                          per-session ordering key = messages.message_id (MAX+1)
  ├── views replay ─────────► SessionView / MessageView  ── two SEPARATE locks from append
  │        (append_and_apply / turn loop: append releases lock, THEN views.write().await)
  │                                   └── REST: GET /api/sessions, /api/messages
  └── state.deltas: broadcast::Sender<ServerFrame>  ── seq-less, unordered, fire-and-forget
        └── /ws ──► should_deliver() drops session-scoped frames for unsubscribed conns
                       │  MessageDone broadcast BEFORE durable append (races refetch)
                       ▼
        TanStack Query cache (staleTime:∞) + ChatPage-local streamParts/optimistic
```

## 4. Proposed state — minimal, buildable, ordered by `message_id`

### 4.1 Durable-first turn completion (primary fix)

Reorder the turn loop so the durable append + view apply
(`sessions.rs:1532-1543`) happen **before** the `MessageDone` broadcast
(`:1516`). Then the `done`-triggered invalidation refetch (`queries.ts:213`)
always observes the committed assistant row. Smallest diff, highest value;
fixes the in-view vanish/reorder outright.

### 4.2 Deliver-on-(re)subscribe

When a client subscribes to a session (`ws.rs:439`, driven by
`ChatPage.tsx:149-155`), the client force-refetches that session's messages
(drop `staleTime: Infinity` for the active session, or invalidate on subscribe).
Returning to a session then always reconstructs the persisted transcript. Fixes
the navigate-away-and-back case with no seq machinery.

### 4.3 Transcript order = `message_id`

The client renders strictly by the existing per-session `message_id`. Optimistic
user messages hold a provisional local id and are re-keyed to the server
`message_id` on their `message.appended` echo; they never sort above a
higher-`message_id` row. This closes the reorder without a global sequence.

### 4.4 Optimistic reconcile by `clientMsgId` (durable, not wire-only)

Add `clientMsgId` to the **durable** `Event::MessageAppended` (not just the wire
frame). Reason (review H8): the `done`-triggered refetch reads the DB
(`queries.ts:213`); if `clientMsgId` lives only on the transient frame, the
refetched rows lack it and cannot dedup the optimistic bubble — the reorder is
only narrowed. This is an append-only-log schema addition; replayed historical
events lack the field, so the reducer treats missing `clientMsgId` as "no
optimistic match" and falls back to content/`message_id` matching.

### 4.5 message_count — single event-derived count

Increment `row.message_count` in `SessionView::apply(MessageAppended)`
(`views/session.rs:201`) and decrement on `MessageRemoved`. **Reconcile the sync
double-count** (review §7.3): `sync.rs:370-375` emits
`SessionUpdated{message_count: <absolute>}`; a session that is both synced and
managed would double-count. Rule: **absolute-set on sync, increment on managed
append, never both for the same message.** Replay test must cover *both* session
kinds.

### 4.6 Optional session-scoped catch-up cursor (only if still needed)

If, after 4.1–4.4, a cursor is still wanted for reconnect robustness, add a
**session-scoped, org-checked** `GET /api/messages?session=<id>&since=<message_id>`
— never reuse the global, unscoped `/api/events` for browser message catch-up
(review H4: it leaks cross-org events). This keeps the catch-up coordinate system
identical to the render key (`message_id`).

## 5. Envoy operational logs (unchanged intent, corrected mechanism)

Envoy diagnostics still need to reach the client, but NOT via a global-seq
envelope. Project them as **session-scoped `session.log` frames** (the variant
already exists, `ws.rs`) for session-bound events, gated by the same
`should_deliver` org/session filter. Full OTel remains ADR 0018's TTL plane; do
not couple product truth to it (review §4: §9 of v1 over-reached).

## 6. What v2 explicitly drops from v1

- `streamEpoch` / `streamId` / global `seq` frame envelope.
- "live seq == log seq" stamping.
- Browser catch-up via `/api/events`.
- Delta-recovery via catch-up (deltas are never logged — review H2).
- Any claim that reconcile is "wire-only" (4.4 is a durable schema change).

## 7. Serialization prerequisite (inherited blocker)

Even this minimal design assumes live projection order matches replay order. It
does **not** today: append and view-apply take separate locks
(`support.rs:22-31`, `sessions.rs:1532/1541`), so two concurrent turns can commit
in one order and apply in another (review H3; project/repo review §4.3). 4.1
(durable-first) removes the *specific* refetch race for a single turn, but the
general append-then-apply hazard remains and must be closed by a single
serialized append+apply critical section before any multi-writer correctness is
claimed. This ADR does not itself fix that seam; it depends on it and flags it.

## 8. Hostile acceptance tests (revised)

- send → navigate A→B→A while streaming → returning shows the completed
  assistant message in correct `message_id` order (4.1 + 4.2);
- `done` arrives → the refetch never misses the assistant row (durable-first);
- an older assistant response can never sort above a newer user message, keyed
  on `message_id`;
- optimistic bubble is de-duped after refetch via durable `clientMsgId`
  (not content-equality), including the `done`→refetch path;
- full page refresh mid-turn reconstructs persisted messages (partial in-flight
  delta text may be lost until `done` — deltas are not durable; this is the
  honest guarantee, not "gap-filled");
- **concurrent-writers test**: two turns committing near-simultaneously apply in
  commit order (guards the §7 seam);
- `message_count` replay test for **both** managed and Hermes-sync sessions
  (no double-count);
- old-server/new-client: frames without `clientMsgId` fall back to the legacy
  invalidate-refetch path (no undefined behavior in a mixed fleet).

Dropped from v1 as untestable/false: delta-recovery via catch-up; streamEpoch
minting; global-seq gap→catch-up.

## 9. Consequences

- The reported bug is fixed by 4.1–4.3 with a small, shippable diff, no new
  coordinate system, and no dependency on the unbuildable global-seq spine.
- Transcript correctness is grounded in the ordering key that already exists
  (`message_id`), so live order and replay order agree by construction for a
  single session.
- The deeper multi-writer serialization hazard (§7) is named as a shared
  prerequisite with the other reviews, not papered over.
- WebSocket retained; WebRTC remains a future transport with no bearing on
  correctness.
