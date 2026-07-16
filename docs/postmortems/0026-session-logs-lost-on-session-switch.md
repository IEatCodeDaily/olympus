# 0026 — Session logs disappeared when switching sessions

**Status:** Fixed and browser-verified against the live Hall  
**Incident:** 2026-07-13

## Impact

The Logs panel lost all visible history whenever the operator selected another session. Returning to the original session showed an empty panel even though Hall still retained the session messages and startup failure.

## Root cause

`BottomPanel` treated diagnostic logs as component-local state and explicitly cleared that state on every `sessionId` change. It did not project retained lifecycle history from Hall's message query.

## Corrective actions

- Reconstruct the durable lifecycle subset from `useMessages(sessionId)`:
  - user-message sent,
  - system lifecycle/error messages,
  - tool name/status,
  - turn finish reason.
- Keep live `session.log` frames in a separate ephemeral tail.
- Merge and deduplicate durable and live entries with stable IDs.
- Reset only live/debug state when the selected session changes.
- Implement Clear as a local timestamp filter; it does not delete Hall truth.
- Deliberately exclude prompt content, reasoning, tool arguments, and tool results from the diagnostics projection.

## Verification

- Projection and redaction unit tests passed.
- Component test switched session A → B → A and verified Hall-retained history rehydrated each time.
- UI TypeScript and production Vite build passed.
- Headless Chromium switched live Hall sessions A → B → A, reopened Logs, and
  found the retained `User message sent` entry after returning to A.
- Desktop and 412×915 screenshots were reviewed. The mobile chat had no
  horizontal overflow; a clipped agent badge found during review was corrected.

## Architecture boundary

This is not full persistent telemetry. It is a view over existing Hall-owned product truth while retained. Live-only adapter details remain ephemeral.

ADR 0018 remains authoritative for complete OpenTelemetry-standard logs, spans, and metrics in the TTL telemetry store, with completeness, expiry, redaction, quotas, source identity, and optional OTLP export. No competing permanent `SessionLog` product store was added.