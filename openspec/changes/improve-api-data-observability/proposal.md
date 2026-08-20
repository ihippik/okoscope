## Why

Runtime Inventory and Runtime Diff are cursor-paginated, so the Web UI cannot derive truthful distributions, totals, or largest changes from a single page. The backend needs bounded full-dataset aggregates and a safe way to navigate from a distribution bucket to the matching typed inventory identity without changing existing pagination semantics.

## What Changes

- Add a tenant-scoped Runtime Inventory distribution endpoint for `process`, `destination`, `domain`, and `syscall`, using the same operational filters as inventory list/summary and computing exact totals across the full filtered set.
- Return a bounded top-N ordered by occurrence count with an exact `other` remainder, inert semantic summaries, stable identity tokens, and deterministic tie-breaking.
- Extend Runtime Inventory list filtering with an opaque, integrity-protected `identity_token` bound to identity version and the trusted Project/Application/kind scope; document stable validation errors for invalid, expired, or incompatible tokens.
- Add a tenant-scoped Runtime Diff summary endpoint with exact full-comparison classification totals and bounded largest changes, reusing existing baseline selection and identity comparison rules.
- Implement database-side aggregation and bounded top-N execution; add supporting indexes only when justified by query plans.
- Expand OpenAPI, contract fixtures, automated correctness/security coverage, and high-cardinality performance evidence, including response-size and p50/p95/p99 measurements.
- Preserve current Runtime Inventory and Runtime Diff cursor pagination behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-runtime-inventory`: Adds full-scope distributions, opaque typed-identity filtering, deterministic remainder accounting, token validation, and bounded aggregation requirements.
- `release-runtime-diff`: Adds full-comparison classification totals and bounded largest-change summaries using existing release ownership, baseline selection, and identity comparison semantics.

## Impact

- Extends authenticated routes, query parsing, service/repository aggregation, and error mapping for Runtime Inventory and Runtime Diff.
- Updates `openapi/okoscope-v1.yaml`, generated/hand-authored API models, fixtures, route/contract checks, and frontend handoff documentation.
- Adds database queries and potentially migrations for indexes supporting high-cardinality filtered aggregation and stable top-N ordering.
- Adds unit, integration, tenant-isolation, contract, and performance tests; performance reports record dataset shape, latency percentiles, maximum response size, query plans, indexes, top-N limit, and known constraints.
- Requires synchronization with the frontend-owned OpenAPI snapshot while retaining backend ownership of authorization and scope validation.
