# Olympus → Hermes patches

Olympus requires a few changes to **Hermes Agent**. We **patch, never fork** —
every change lives here as a reviewable `git`-format `.patch` file, committed to
the Olympus repo, so that:

- **All changes are visible in one place** (this directory + `manifest.toml`).
- They are **re-appliable** after `hermes update` (which autostashes and silently
  drops working-tree edits — see the `cli-startup-performance` skill).
- They are **reviewable as diffs**, not buried as ad-hoc working-tree edits.
- We never carry a divergent fork; upstream stays upstream, our delta is explicit.

## Scope

This registry is for **Olympus-required Hermes changes only** — changes Olympus
depends on to function. It is NOT the place for the operator's general
dev-environment Hermes patches (WSL startup speed, TUI fixes, billing bypass) —
those are tracked in the `cli-startup-performance` skill. Keep the two separate:
Olympus's patches ship with the product; the dev-env patches are host-local.

## Target

- Upstream: `IEatCodeDaily/hermes-agent` (a fork on GitHub; locally a git
  checkout at `~/.hermes/hermes-agent`).
- Override the checkout path with `HERMES_AGENT_DIR`.
- The upstream commit each patch was authored against is recorded in
  `manifest.toml` (`base_commit`) for drift detection.

## Workflow

```bash
# See what's in the registry + whether each applies / is already applied / drifted
./patchctl.sh status

# Dry-run: would every patch apply cleanly to the current checkout?
./patchctl.sh check

# Apply all registry patches (idempotent — skips already-applied)
./patchctl.sh apply

# Capture a change you made in the Hermes checkout as a NEW patch
#   (edit files in ~/.hermes/hermes-agent first, then:)
./patchctl.sh save 001-sessiondb-create-fork hermes_state.py
#   → writes patches/001-sessiondb-create-fork.patch from the working-tree diff
#   → then add it to manifest.toml and commit to the Olympus repo
```

### After `hermes update`

`hermes update` resets the checkout to upstream and stashes local edits. To
restore Olympus's patches: `./patchctl.sh apply`. If a patch no longer applies
cleanly (upstream moved the code), use `git -C ~/.hermes/hermes-agent apply
--3way patches/<file>` to do a 3-way merge, re-save the resolved diff with
`patchctl save`, and bump `base_commit` in the manifest.

## Conventions

- **Numbered, ordered:** `NNN-short-slug.patch`, applied in manifest array order.
- **One concern per patch:** a patch should be reviewable in isolation.
- **Prefer additive, structural changes** (new methods/functions) over edits to
  hot upstream code — they survive upstream drift better.
- **Every patch needs a `manifest.toml` entry** stating its purpose, the
  Olympus feature that needs it (ADR §ref), and the files it touches.
- **Verification:** a patch entry should name the Hermes test(s) that prove it
  works, so re-application can be validated, not assumed.

## Planned patches

| # | Patch | Purpose | Status |
|---|---|---|---|
| 001 | `sessiondb-create-fork` | `SessionDB.create_fork()` — atomic, invariant-correct session fork (source=`acp`, `_branched_from`, counters, FTS, lineage) for Olympus cross-channel resume (ADR 0002 §6.6) | **pending fork-spike** (must be proven on a COPIED state.db first — adversarial review blocker #2) |

No patch files exist yet — `create_fork` is gated on the fork spike. This commit
establishes the system; the first patch lands after the spike.
