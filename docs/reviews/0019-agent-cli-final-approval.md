# ADR 0019 final approval review

> **Superseded:** this focused review covered only the narrow request stated at
> dispatch and predated the full B1-B8 review in
> `0019-agent-cli-adversarial-review.md`. It is retained as review history and is
> not the architecture's final verdict; the adversarial rereview controls.

## Verdict: APPROVED

No approval-blocking contradiction was found in the requested scope.

- **Security and identity:** Agent access is restricted to a private runtime gateway bound to one node/runtime-attempt/session identity; Hall derives identity and reauthorizes every call, while agent-selected credentials, endpoints, and profiles are forbidden (`docs/adrs/0019-agent-and-human-cli-interface.md:182-203,269-278`; `docs/plans/2026-07-13-session-cutover-remote-development.md:301-324,336-341`).
- **Idempotency and retry:** Effectful requests allocate a client request ID before first send, reuse it across reconnect/retry, and resolve to the original durable resource; the black-box gate requires ambiguous response loss to create exactly one run (`docs/adrs/0019-agent-and-human-cli-interface.md:146-151,269-283,334-336`; `docs/plans/2026-07-13-session-cutover-remote-development.md:462-467`).
- **Dynamic-schema TOCTOU:** Parsing pins the active immutable definition digest, submits that digest, and requires Hall to validate and start exactly that retained definition or conflict; activation substitution is explicitly tested (`docs/adrs/0019-agent-and-human-cli-interface.md:79-91,323-336`; `docs/plans/2026-07-13-session-cutover-remote-development.md:455-466`).
- **CLI pipelines and output:** stdout is reserved for selected machine/result data, progress and diagnostics use stderr, failed `result-json` emits no stdout, broken pipes detach without cancellation, and the real credential-free pipeline is a gate (`docs/adrs/0019-agent-and-human-cli-interface.md:132-180,248-267`; `docs/plans/2026-07-13-session-cutover-remote-development.md:343-364,455-469`).
- **Dependency order:** Phase 3 exposes only non-effectful `session.info`; effectful workflows wait for PKG-1, hardened capabilities, durable jobs, providers, the runtime gateway, CLI core, and—when effectful—the hostile sandbox gate, matching ADR 0019's no-early-exposure gate (`docs/adrs/0019-agent-and-human-cli-interface.md:323-341`; `docs/plans/2026-07-13-session-cutover-remote-development.md:326-341,443-445`).
