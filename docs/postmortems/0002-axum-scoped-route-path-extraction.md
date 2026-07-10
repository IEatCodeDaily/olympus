# Postmortem: organization route proxy retained outer Axum path parameters

- Date: 2026-07-10
- Severity: development-only
- Status: resolved

## Summary

The first organization-scoped route implementation nested the existing resource router below `/api/organizations/{organization_id}`. Existing handlers used extractors such as `Path<String>` and expected only their resource identifier. Axum retained the outer `organization_id` matched parameter, so detail handlers received a different extractor shape and returned `500`.

Changing only the forwarded request URI did not fix the problem because Axum stores matched path parameters in request extensions as well as in the URI.

## Detection

A focused integration test requested an existing session through the new scoped detail URL. Collection requests passed, but the detail request returned `500` instead of `200`, proving the alias was not contract-compatible.

## Root cause

We treated URI nesting as a transparent prefix operation. It is not transparent to Axum extractors: outer matched parameters remain part of request state consumed by the inner handler.

## Resolution

The scoped boundary now uses one catch-all adapter. It:

1. authorizes organization membership before dispatch;
2. extracts the organization and resource suffix;
3. rebuilds a fresh HTTP request with method, rewritten URI, version, headers, and body;
4. deliberately drops outer Axum request extensions;
5. inserts a typed `OrganizationScope` extension; and
6. dispatches into the legacy resource router.

Focused collection and detail tests exercise the adapter. Resources are registered in the scoped router only after their handlers enforce durable organization ownership.

## Prevention

- Test both collection and detail routes whenever introducing an Axum route prefix.
- Include multi-segment mutations and query strings in scoped-route coverage.
- Do not infer tenant isolation from route nesting; require a typed scope plus resource-owner filtering.
