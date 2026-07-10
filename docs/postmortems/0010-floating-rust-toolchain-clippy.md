# Postmortem 0010: floating Rust toolchain exposed new Clippy lints

## Summary

The consolidated `main` passed the host's Rust 1.96 Clippy gate but failed GitHub Actions on Rust 1.97. The workflow tracks `stable`, and the newer release enabled two additional warn-by-default lints that become errors under `-D warnings`.

## Impact

All tests and UI CI passed, but the canonical Rust job remained red after the first consolidated push.

## Root cause

Local and CI verification did not use the same Rust release. Two harmless patterns—an explicitly ignored field next to `..` and a borrowed formatting argument—were accepted by Rust 1.96 and rejected by Rust 1.97.

## Resolution

Both patterns were simplified so they pass on both releases. No lint was suppressed.

## Prevention

- Record the compiler version in verification evidence.
- Either pin one Rust release across development and CI or test the minimum supported release plus current stable.
- Continue denying warnings, but treat stable-toolchain movement as an explicit compatibility event.
