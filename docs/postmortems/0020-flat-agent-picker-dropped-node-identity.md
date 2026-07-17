# Postmortem 0020: New Session hid same-id agents on other nodes

## Summary

The New Session picker consumed the flat `/api/agents` list, which deduped agents by id and dropped node identity. Agents on another node with the same id as a local profile disappeared from the picker, and even unique remote agents could not be targeted because session creation sent only `{agent}`.

## Impact

- Operators could not deliberately start a new session on a specific enrolled node.
- Same-id remote agents were hidden behind the local entry.
- Draft creation fell back to implicit runtime routing instead of preserving the operator's node choice.

## Root cause

`NodeRegistry::all_agents()` intentionally returns the legacy flat shape and dedupes by `agent.id` in a `BTreeMap`. That is fine for old consumers, but the New Session flow needed `(node, agent)` availability. The UI treated agent ids as globally unique and called `createSession({ agent })`, so the selected runtime was under-specified.

## Detection

The bug was reported from the picker: Fx-ZephyrusM16 agents were missing. Code audit confirmed the flat `/api/agents` route and the `createSession({ agent })` call path.

## Resolution and prevention

- Added `/api/agents/catalog` backed by `NodeRegistry::agent_catalog()` so duplicate agent ids survive as separate per-node entries.
- Rewrote `AgentPicker` around per-node groups, offline gating, search, keyboard selection, and an Often selected section derived from recent `(agent,node)` session pairs.
- Changed the sidebar create path to call `createSession({ agent, node })`.
- Made explicit-node session creation fail closed for unknown/offline nodes.
- Added Rust and UI regression tests for duplicate ids, often-selected derivation, offline gating, and explicit node selection.
