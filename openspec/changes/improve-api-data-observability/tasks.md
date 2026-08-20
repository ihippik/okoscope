## 1. Contract and Shared Scope

- [x] 1.1 Compare backend routes and models with the frontend `openapi/okoscope-v1.yaml` source contract and record any naming or schema differences to resolve
- [x] 1.2 Add concrete OpenAPI parameters, response schemas, examples, bearer security, and 400/401/404 errors for inventory distribution and runtime diff summary
- [x] 1.3 Add `identity_token` length bounds and document `invalid_identity_token`, `expired_identity_token`, and `identity_token_scope_mismatch` in Runtime Inventory list OpenAPI
- [x] 1.4 Refactor Runtime Inventory list, summary, and distribution to use one normalized filter/scope representation without changing existing cursor semantics
- [x] 1.5 Refactor Runtime Diff list and summary to share owned release resolution, default baseline selection, and typed identity comparison rules

## 2. Protected Typed Identity Tokens

- [x] 2.1 Implement a bounded versioned identity-token payload and canonical serialization for identity version, kind, canonical identity, trusted scope, issuance, and expiry
- [x] 2.2 Protect tokens with the repository's approved authenticated signing/encryption key facility and add startup validation plus key-rotation/grace behavior where supported
- [x] 2.3 Implement validation that derives tenant, Project, Application, kind, and identity version independently and maps malformed/tampered, expired, and wrong-scope cases to the documented 400 codes
- [x] 2.4 Apply the validated typed identity as an additional repository predicate alongside every active Runtime Inventory filter
- [x] 2.5 Add unit tests for round trips, length bounds, tampering, expiry, wrong tenant/Project/Application/kind/version, and token/cursor independence

## 3. Runtime Inventory Distribution

- [x] 3.1 Implement database-side grouped inventory aggregation for process, destination, domain, and syscall using the complete normalized filtered scope
- [x] 3.2 Compute exact total item/occurrence counts, deterministic top-N ordering, and `other` remainder in one statement or consistent read snapshot
- [x] 3.3 Implement kind-specific bounded semantic summaries and token issuance while preserving hostile or markup-like observed strings as inert JSON text
- [x] 3.4 Add the authenticated tenant-safe distribution service and route with kind validation and limit default 5/range 1..10
- [x] 3.5 Add integration tests for every kind, each filter and representative combinations, search scope, empty data, with/without `other`, tied counts, limits 0/1/5/10/11, and exact full-set accounting
- [x] 3.6 Add ownership and standard correlated 400/401/404 tests for Project/Application mismatches and invalid filters

## 4. Runtime Diff Summary

- [x] 4.1 Implement database-side full-comparison classification aggregation for `new`, `disappeared`, and identity-preserving `unchanged`
- [x] 4.2 Compute baseline/target counts and signed deltas, then select bounded largest changes by absolute delta descending and group ID ascending
- [x] 4.3 Add the authenticated tenant-safe summary service and route with explicit/default baseline resolution and limit default 5/range 1..10
- [x] 4.4 Return `baseline: null`, zero totals, empty classifications, and empty largest changes when backend baseline selection finds none
- [x] 4.5 Add integration tests for datasets larger than a diff page, all classifications, positive/negative/zero deltas, stable ties, explicit/default/missing baseline, limits 0/1/5/10/11, and exact total consistency
- [x] 4.6 Add ownership and correlated 400/401/404 tests for path Project/Application/target and explicit baseline mismatches

## 5. Query Performance and Observability

- [x] 5.1 Build reproducible high-cardinality fixtures covering many inventory identities, wide time ranges, release/Kubernetes filters, and many diff groups
- [x] 5.2 Capture query plans for all inventory kinds and diff summary, identify scans/sorts, and add only indexes justified by measured plans
- [x] 5.3 Verify aggregation returns only totals plus bounded top-N rows to application memory and enforce existing statement cancellation/time budgets
- [x] 5.4 Run repeated load checks and record dataset volume, p50/p95/p99, maximum serialized response size, query plans, used indexes, top-N maximum 10, and known limitations
- [x] 5.5 Add bounded route/status latency and outcome telemetry without token, tenant, raw-search, request-ID, or semantic metric labels

## 6. Contract Fixtures and Verification

- [x] 6.1 Add schema-aligned response examples and fixtures for non-empty, empty, remainder, no-remainder, no-baseline, and hostile semantic-string cases
- [x] 6.2 Add route inventory and OpenAPI contract tests for operation IDs, parameters, concrete schemas, examples, authentication, request IDs, and token error envelopes
- [x] 6.3 Run backend unit, integration, contract, migration, and formatting/lint test suites and resolve regressions
- [x] 6.4 Verify existing Runtime Inventory and Runtime Diff pagination ordering, cursor validation, and response behavior remain unchanged
- [x] 6.5 Synchronize the reviewed backend OpenAPI contract to the frontend snapshot, regenerate frontend schema types, and verify generation leaves no drift
- [x] 6.6 Deliver example responses for both new endpoints, the three exact identity-token error codes, and the recorded performance results to the frontend team
