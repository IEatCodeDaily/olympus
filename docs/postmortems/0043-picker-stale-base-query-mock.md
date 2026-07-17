# 0043 — Node-aware picker branch passed stale-base tests but broke current-main sidebar tests

Date: 2026-07-17 · Severity: medium (integration gate failure) · Author: Terminus

## Symptom

After merging the node-aware AgentPicker branch into current main, all three
`SessionSidebar` tests crashed before their assertions. Vitest reported that the
mocked `hooks/queries` module had no `useAgentCatalog` export. The merged Rust
tests then failed to compile because their stale `AgentInfo` initializers omitted
the newer `models` field.

## Root cause

The feature branch verified against an older base whose sidebar test did not
exist in its current form. The new AgentPicker is permanently mounted (queries
are disabled while closed) and therefore calls `useAgentCatalog` during every
sidebar render. Current main's focused module mock still exposed the old
`useAgents` hook. The branch also predated the model-catalog addition to
`AgentInfo`. Branch-local tests passed, but the merged dependency surface did not.

## Fix

The current-main sidebar test now mocks `useAgentCatalog` with the real response
shape (`{ data: { nodes: [] }, isLoading: false }`) and removes the obsolete
`useAgents` mock. Both Rust test helpers now initialize `models` explicitly.

## Prevention

- UI branches that touch a permanently mounted child must rebase onto current
  main and run the full current-main suite before review completion.
- Integration review must inspect module mocks in sibling tests, not only the
  feature's new test file.
