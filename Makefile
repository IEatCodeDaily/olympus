# Olympus — canonical developer commands.
#
# This is a Rust workspace (control plane) + a Vite/React UI under ui/.
# The single source of truth for "is the tree green?" is `make verify`.

SHELL := /bin/bash
.PHONY: verify verify-rust verify-ui test lint fmt build run e2e e2e-desktop e2e-live

## verify — run ALL canonical gates (Rust + UI). The harness's go-to command.
verify: verify-rust verify-ui
	@echo "ALL CANONICAL GATES GREEN"

## verify-rust — cargo test + clippy (-D warnings) + fmt --check
verify-rust:
	cargo test --workspace
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

## verify-ui — typecheck + build + Playwright e2e desktop (fast inner loop)
verify-ui:
	cd ui && npx tsc --noEmit
	cd ui && npx vite build
	cd ui && npx playwright test --project=chromium-desktop --reporter=line

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

## e2e — full e2e (desktop+mobile) with evidence bundle (videos + screenshots)
e2e:
	cd ui && npx playwright test
	cd ui && bash scripts/evidence-bundle.sh

## e2e-desktop — fast inner loop, desktop only, no bundle
e2e-desktop:
	cd ui && npx playwright test --project=chromium-desktop

## e2e-live — smoke tests against the REAL control plane (spends tokens)
e2e-live:
	cd ui && npx playwright test --config=playwright.live.config.ts
