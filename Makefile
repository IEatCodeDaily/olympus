# Olympus — canonical developer commands.
#
# This is a Rust workspace (control plane) + a Vite/React UI under ui/.
# The single source of truth for "is the tree green?" is `make verify`.

SHELL := /bin/bash
.PHONY: verify verify-rust verify-ui test lint fmt build run

## verify — run ALL canonical gates (Rust + UI). The harness's go-to command.
verify: verify-rust verify-ui
	@echo "ALL CANONICAL GATES GREEN"

## verify-rust — cargo test + clippy (-D warnings) + fmt --check
verify-rust:
	cargo test --workspace
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

## verify-ui — typecheck + build + Playwright e2e (run from ui/)
verify-ui:
	cd ui && bun run typecheck
	cd ui && bun run build
	cd ui && bunx playwright test --reporter=line

## test — Rust tests only (fast inner loop)
test:
	cargo test --workspace

## lint — clippy with warnings as errors
lint:
	cargo clippy --all-targets -- -D warnings

## fmt — apply rustfmt
fmt:
	cargo fmt

## build — release binary
build:
	cargo build --release

## run — start the control plane (imports state.db, serves API on :8787)
run:
	cargo run --release
