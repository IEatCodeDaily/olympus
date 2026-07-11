# Playwright to Maestro Migration Implementation Plan

**Goal:** Replace Olympus browser E2E execution with Maestro.dev while preserving deterministic mock, live-agent, and production-parity tiers.

**Architecture:** Maestro owns user-visible browser journeys through YAML web flows. A single runner starts an isolated strict-port Vite/MSW server for mock tests, while live and production tiers target operator-provided Hall URLs. HTTP-only production checks move to curl so UI automation is not used as an API test client.

**Tech stack:** Maestro CLI 2.6.1, Java 21, Chromium web driver, Vite/MSW, shell, GitHub Actions.

## Delivery

1. Add `.maestro/` workspace configs and desktop/mobile mock plus live/prod flows using visible text and accessibility-first selectors.
2. Add a pinned, checksum-verified Maestro installer and one fail-closed test runner with JUnit/debug/artifact output.
3. Replace npm, Make, CI, ignore, and agent-map Playwright wiring with Maestro commands.
4. Remove Playwright configs/specs only after converting their user-journey intent; keep API assertions in a dedicated production smoke script.
5. Run the mock suite headlessly, then typecheck, Vitest, build, and canonical verification.
6. Review the final diff for lost coverage, stale Playwright references, server leaks, and concurrent-work protection.

## Known trade-offs

- Maestro web is beta and Chromium-only.
- DOM interception/count assertions do not belong in Maestro; exact request and reducer behavior remains in unit/Rust integration tests.
- Mock tests use a checkout-derived strict port and never reuse an existing server.
- Linux hosts with restricted unprivileged user namespaces need the narrow AppArmor profile installed by `ui/scripts/install-maestro-apparmor.sh`.
