# Postmortem 0008: state.db recent-message column mismatch

## Summary

`StateDbReader::recent_messages` computed a message sequence in its inner SQL query but omitted that sequence from the outer `SELECT`. The Rust row decoder nevertheless treated column 0 as the numeric message ID and decoded every later field at the wrong offset. Reads could therefore fail with a SQLite type error before returning any history.

## Impact

Lazy history reads backed by Hermes `state.db` could not reliably return recent messages. The canonical Rust test suite did not expose the defect because this reader had no direct row-mapping regression test.

## Root cause

The query and decoder were changed independently during the lazy-history work. A second loop then replaced IDs with a window-local counter, masking the intended SQL identity semantics in code review while also triggering a new Clippy `explicit_counter_loop` failure after the toolchain update.

## Resolution

The outer query now selects the computed `seq` column. The decoder uses the matching column offsets, preserves the stable session-relative IDs produced by SQL, and collects rows directly without a second counter loop.

A regression test creates a representative `state.db`, requests a limited recent window, and asserts message IDs, content, timestamps, and token counts.

## Prevention

- Keep SQL projection order and row decoding covered by an integration-style unit test.
- Preserve durable/session-relative identifiers rather than replacing them with window-relative counters.
- Run Clippy with warnings denied in the canonical gate.
