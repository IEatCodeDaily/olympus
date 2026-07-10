# Postmortem 0007 — Iroh integration tests depended on public DNS and relays

Date: 2026-07-10
Status: resolved

## Impact

The Rust workspace suite failed on hosts and GitHub runners that could not resolve Iroh's public endpoint-discovery TXT records. The Hall/Envoy transport code was healthy, but two loopback integration tests reported `No addressing information available` before exercising the protocol.

## Root cause

The tests created endpoints with the production N0 preset and connected with only the Hall public key. That forced address lookup through public DNS and could also involve public relays, despite both test endpoints running in the same process.

## Resolution

The integration harness now disables relays and passes the Hall endpoint's concrete `EndpointAddr` directly to the Envoy endpoint. The tests still exercise real encrypted Iroh QUIC streams, allowlist enforcement, Hello registration, and the EnsureRuntime response path without depending on external discovery infrastructure.

## Prevention

Integration tests for transport semantics must provide deterministic local addressing. Public discovery and relay reachability belong in explicit live/network tests, not the canonical offline workspace suite.
