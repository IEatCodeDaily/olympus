# Olympus — canonical developer commands.
#
# This is a Rust workspace (control plane) + a Vite/React UI under ui/.
# The single source of truth for "is the tree green?" is `make verify`.

SHELL := /bin/bash
.PHONY: verify verify-rust verify-ui test lint fmt build run e2e e2e-desktop e2e-live e2e-prod deploy deploy-hall deploy-envoy

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

## e2e-prod — prod-parity: static UI served by the control plane itself
## (same origin cloudflared sees). Requires olympus.service running.
e2e-prod:
	cd ui && npx playwright test --config=playwright.prod.config.ts

## deploy — install both hall + envoy binaries (symlink flip, no restart).
deploy:
	bash scripts/deploy.sh both

## deploy-hall — build hall → symlink flip → restart olympus-hall.service.
## Envoys survive (ADR §5 Hall deploy story); they buffer through downtime.
deploy-hall:
	bash scripts/deploy.sh hall
	systemctl --user restart olympus-hall
	@echo "Hall restarted. Envoys will re-attach on their next reconnect."

## deploy-envoy N — build envoy → symlink flip → start olympus-envoy@N →
## poll /api/nodes until the new envoy is online → drain the old envoy if
## one exists. Usage: make deploy-envoy N=2
deploy-envoy:
	@if [ -z "$$N" ]; then echo "Usage: make deploy-envoy N=2" >&2; exit 1; fi
	bash scripts/deploy.sh envoy
	systemctl --user start olympus-envoy@$$N
	@echo "Started olympus-envoy@$$N, polling /api/nodes until online…"
	@TOKEN=$$(cat ~/.olympus/token); \
	for i in $$(seq 1 30); do \
		online=$$(curl -sf -H "Authorization: Bearer $$TOKEN" \
			http://127.0.0.1:8799/api/nodes \
			| grep -c "envoy-$$N" || true); \
		if [ "$$online" -gt 0 ]; then \
			echo "envoy-$$N is online"; exit 0; \
		fi; \
		sleep 1; \
	done; \
	echo "ERROR: envoy-$$N did not come online in 30s" >&2; exit 1
