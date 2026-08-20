## Context

Runtime Inventory and Runtime Diff already expose deterministic cursor-paginated detail collections. The Web UI now needs charts and top-change cards whose values represent the complete selected scope; aggregating a fetched page would be incorrect. The backend owns tenant authorization, typed identity rules, baseline selection, and the source OpenAPI contract. Queries must remain bounded at the API and application layers even for high-cardinality observation data.

## Goals / Non-Goals

**Goals:**

- Produce exact full-scope inventory distributions and diff classification totals with bounded top-N payloads.
- Keep filter, identity comparison, baseline selection, authorization, and error behavior consistent with existing endpoints.
- Make distribution-to-list navigation safe through opaque scoped identity tokens.
- Establish measurable query-plan, latency, response-size, and index evidence before acceptance.

**Non-Goals:**

- Changing Runtime Inventory or Runtime Diff cursor pagination or list ordering.
- Returning arbitrary chart definitions, presentation markup, or client-rendered links.
- Loading complete aggregate inputs into application memory or introducing an external analytics service.
- Making identity tokens a durable public identifier or a substitute for authorization.

## Decisions

### Aggregate over a shared normalized scope

Inventory list, summary, and distribution will use one normalized filter object and repository scope builder. Diff list and summary will likewise share target/baseline resolution and the typed identity comparison relation. Database CTEs or equivalent query-builder subqueries will form the scoped identity set once, aggregate totals, and select top-N rows.

This prevents semantic drift and page-derived totals. Separate endpoint-specific query implementations were rejected because filter and comparison behavior would diverge over time.

### Compute exact totals and top-N in the database

Inventory queries will group by the canonical typed identity key, compute per-entry item and occurrence counts, derive totals from the grouped relation, select `limit` rows in deterministic order, and calculate `other` by subtraction. Diff queries will form the full outer presence comparison, aggregate classifications, calculate signed deltas, and select largest changes by `ABS(delta) DESC, group_id ASC`.

The application receives totals plus at most ten entries. Fetch-all-and-sort was rejected because memory and response work would scale with cardinality. Approximate sketches were rejected because the UI contract requires exact accounting.

### Issue authenticated, versioned identity tokens

The token payload will carry a token format version, `identity_version`, typed kind, canonical identity key/reference, trusted scope identifiers, issued-at, and expiry. It will be serialized canonically and protected with an existing server key using authenticated signing/encryption. Validation order will bound length and decode work, authenticate the token, enforce expiry/version, compare scope against independently authorized URL and query context in constant-time where relevant, then bind the canonical identity as an additional SQL predicate.

Errors are stable 400 codes: `invalid_identity_token` for malformed/tampered/unresolvable identities, `expired_identity_token` for elapsed validity, and `identity_token_scope_mismatch` for incompatible tenant/Project/Application/kind/version scope. Signed plaintext was preferred over database token rows to avoid lifecycle storage and lookup overhead; raw identity JSON was rejected because clients could tamper with scope or depend on internal identity structure.

### Treat semantic summaries as typed inert data

Summary construction will reuse the same allowlisted per-kind/event-kind projections as existing inventory and diff entries. Values remain JSON strings with schema bounds; the backend creates no HTML, Markdown, hyperlinks, or arbitrary navigation targets.

### Enforce cost at several layers

Both endpoints enforce `limit` 1..10 with default 5, bounded filters/search, statement timeout/cancellation inherited from API data access, and a concrete response schema. Query plans on representative high-cardinality fixtures determine whether composite/partial indexes on tenant/application, release/scope/time, typed identity, and occurrence aggregates are required. Metrics use route/status buckets without tenant, token, raw search, request ID, or semantic labels.

### Keep backend OpenAPI authoritative

The backend contract will define parameters, schemas, examples, and standard 400/401/404 envelopes including token codes. Contract tests verify route coverage and fixtures, after which the frontend snapshot is synchronized and generated types refreshed.

## Risks / Trade-offs

- [Exact wide-range aggregation can remain expensive at high cardinality] → constrain filters and top-N, inspect real query plans, add justified indexes, apply statement budgets, and record measured limits.
- [A token signing-key rotation can invalidate active tokens] → document short validity and stable invalid/expired errors; if existing key-ring support exists, validate with active and grace keys.
- [Lexical token ordering may be stable but not semantically meaningful] → use it only as the documented distribution tie-breaker and never expose payload meaning.
- [Concurrent ingestion can make separately selected totals and entries inconsistent] → compute each response in one database statement or one consistent read snapshot.
- [Shared filter refactoring can regress list behavior] → preserve list cursor semantics and add equivalence/contract tests around the common scope builder.

## Migration Plan

1. Add any query-supporting indexes with online/concurrent migration behavior supported by the deployed database.
2. Deploy token configuration/key validation before enabling the new routes; fail startup on missing production key material.
3. Ship repository aggregation, services, handlers, concrete OpenAPI schemas/examples, and automated tests.
4. Run high-cardinality performance checks and retain dataset, percentile, response-size, plan, index, and limitation evidence.
5. Synchronize the reviewed OpenAPI document into `okoscope-web`, regenerate types, and communicate the three identity-token error codes.
6. Roll back handlers independently if necessary; additive routes/indexes and the optional list parameter leave existing clients and pagination behavior intact.

## Open Questions

- Which existing application secret/key-ring facility should own identity-token signing and rotation?
- What token validity period balances dashboard navigation with prompt key/scope invalidation?
- Do representative query plans justify new indexes or can current runtime summary indexes satisfy the acceptance targets?
