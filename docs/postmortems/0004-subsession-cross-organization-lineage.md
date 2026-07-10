# Postmortem 0004: Subsession lineage lacked organization validation

## Summary

The organization-scoped router validated the session ID in the request path,
but the subsession list and completion handlers trusted projected
`parent_session_id` links without proving that both linked sessions belonged to
the same organization.

## Impact

Normal creation paths inherit the parent's organization, so valid data was not
exposed. However, imported, corrupted, or future incorrectly-written lineage
events could have caused:

- a child from another organization to appear in a parent's subsession list;
- completing a scoped child to append a system message to a parent in another
  organization.

That violates the fail-closed cross-reference requirement.

## Root cause

Authorization stopped at the URL resource. The secondary parent/child
relationship was treated as structurally trustworthy rather than as another
tenant-owned reference requiring validation.

## Resolution

- Subsession listing now requires each child organization to match the resolved
  parent organization.
- Completion resolves the parent and returns `404` when it is absent or belongs
  to a different organization, before writing any message or archive event.

## Prevention

Every scoped mutation and relationship traversal must validate both ends of a
reference against durable organization ownership. URL membership and ownership
checks are necessary but not sufficient for secondary resources.
