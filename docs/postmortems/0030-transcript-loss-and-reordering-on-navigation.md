# Postmortem 0030 — Transcript loss and reordering on session navigation

- Date: 2026-07-14
- Severity: high (user-visible data-integrity defect: responses lost, then
  displayed out of order)
- Status: root-caused; fix specified in ADR 0020, implementation pending
- Related: ADR 0020 (client state projection), postmortem 0026 (BottomPanel
  session-log loss — narrower, already fixed)

## Symptom

1. User sends a message.
2. User navigates to another view/session.
3. User returns; the agent's response is absent.
4. User sends a second message.
5. The earlier assistant response then appears **above** the newly-sent message.

## Root cause

Three faults, all confirmed in source; none alone explains it, together they
produce the exact symptom.

1. **Two disconnected truth paths.** The durable ordered spine is the
   append-only event log with monotonic `seq` (`crates/control-plane/src/log.rs`,
   `GET /api/events?since=`). The live stream is a *separate* seq-less,
   unordered `broadcast::Sender<ServerFrame>` (`server/mod.rs:93`). Live frames
   carry no position and cannot be reconciled against the log.
2. **Frames dropped for unsubscribed sessions.** `should_deliver`
   (`server/ws.rs:281`) withholds session-scoped frames from any connection not
   subscribed to that session. Navigating away unsubscribes, so the turn's
   `message.appended`/`message.done` are never delivered to that connection and
   never replayed on return.
3. **No client convergence.** `useMessages` is `staleTime: Infinity`
   (`ui/src/hooks/queries.ts:77`); `message.delta` is ignored globally
   (`:208-211`); `message.appended`/`done` only *invalidate*
   (`:212-221`); `ChatPage` keeps the live turn in component-local
   `streamParts` + optimistic state and resets it on session change. No
   watermark, no gap detection, no snapshot reconciliation.

Sequence of events producing the symptom: the assistant message commits to the
log while the client is unsubscribed (frame dropped). On return, the
`staleTime:Infinity` query is not refetched, so the message is absent. The next
send invalidates the messages query; the refetch now returns the earlier
assistant message, which React renders in server order — above the just-sent
optimistic user message that has no committed seq yet — so it appears "above"
the new message.

Server-side corroboration: managed-session rows report `message_count = 0`
despite durable `messages` rows, because `SessionView::apply(MessageAppended)`
(`views/session.rs:201-211`) advances only `last_activity` and never increments
the count, while `MessageView.counts` (`views/message.rs:85`) does — two
projections of the same event disagree. This proves the divergence is not
purely client-side.

## Fix (ADR 0020)

Make the WS stream an ordered projection of the event log and make the client a
deterministic fold over it:

- sequence every non-presence `ServerFrame` with `{streamEpoch, streamId, seq}`,
  contiguous per stream;
- REST snapshots return the `seq` they reflect;
- client holds a per-stream watermark; applies `seq == watermark+1`, drops
  `<= watermark`, and on a gap (`> watermark+1`) pauses live and catches up via
  `GET /api/events?since=watermark`;
- deliver-on-(re)subscribe replaces subscription-drops-frames, so returning to a
  session reconstructs the turn;
- `message.done` is durable-first (assistant row + seq committed before the turn
  is reported complete);
- optimistic sends reconcile by `clientMsgId`; transcript order is always server
  `seq`;
- `ChatPage` stops owning transcript truth;
- collapse `message_count` to a single event-derived increment.

## Lessons

- A "success" response (201/202, or a live frame) must mean the desired state is
  both **durable** and **ordered**, or the client cannot converge. All three
  adversarial reviews (agent/session, terminal, project/repo) independently
  named this same missing guarantee.
- UI correctness is a state/protocol property (snapshot + version + sequence +
  reconciliation), not a transport property. WASM/WebRTC would not have fixed
  it; sequencing the existing WebSocket stream does.
- Never keep transcript truth in component-local React state that resets on
  navigation. The cache must be a projection the client provably catches up to.
