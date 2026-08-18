## 1. Shared Filter Semantics

- [x] 1.1 Extract one normalized runtime-inventory filter model shared by list, summary, and facets, including bounded validation and a stable scope fingerprint for cursors
- [x] 1.2 Refactor the existing inventory list to use the shared filter model without changing response shape, ordering, tenant isolation, or allowlisted semantic-search behavior
- [ ] 1.3 Add unit tests for normalization, invalid time ranges, search bounds, supported kinds, tenant-scoped release filters, and deterministic filter fingerprints

## 2. Scoped Inventory Summary

- [x] 2.1 Extend the summary query and handler with release, cluster, namespace, workload kind, workload name, container name, observation-window, and semantic-search filters
- [x] 2.2 Ensure summary returns all four kind aggregates from the same matching item set as an unqualified-kind list request and preserves Application-wide behavior without filters
- [ ] 2.3 Add integration tests comparing filtered summary totals and time bounds with matching list results, including empty scope, invalid filters, and cross-tenant release identifiers

## 3. Bounded Facet API

- [x] 3.1 Add closed facet parsing and typed response models for cluster, namespace, workload kind, workload name, and container name values, labels, item counts, and occurrence counts
- [x] 3.2 Implement the authenticated `/runtime-inventory/facets/{facet}` route with bounded option search, maximum 200-item pages, deterministic ordering, and opaque cursors
- [x] 3.3 Apply kind, release, time, semantic search, and all deployment filters except the requested facet's own selected value
- [x] 3.4 Bind facet cursors to facet name and normalized effective scope and reject malformed, cross-facet, cross-Application, or changed-filter cursor reuse
- [ ] 3.5 Deduplicate item and occurrence aggregates across overlapping sightings and add integration tests for multi-Pod, multi-workload, and multi-cluster evidence
- [ ] 3.6 Add tenant-isolation, unsupported-facet, option-search, pagination stability, empty page, and 200-item response-bound tests

## 4. Query Performance and Operations

- [ ] 4.1 Capture representative `EXPLAIN (ANALYZE, BUFFERS)` plans for scoped summary and every facet at documented item, release, Pod, and distinct-value cardinalities
- [ ] 4.2 Add only measured projection indexes required to keep summary and facet queries within documented development regression ceilings, with migration tests if schema changes
- [ ] 4.3 Extend low-cardinality metrics and structured logs for summary/facet request duration, result size, validation failure class, and cursor rejection without logging search or observed values
- [ ] 4.4 Update runtime inventory performance and operations documentation with facet cardinality assumptions, recorded measurements, rollout checks, and rollback behavior

## 5. Evidence Navigation Safety

- [x] 5.1 Centralize generation of item evidence hints from trusted Project, Application, and item identifiers and four closed child-route suffixes
- [x] 5.2 Add tests proving every emitted hint is root-relative, exact-scope, query-free, fragment-free, authority-free, traversal-free, and unaffected by observed semantic or Kubernetes values
- [ ] 5.3 Update OpenAPI and frontend handoff to make typed route construction canonical and returned evidence hints optional validated conveniences rather than arbitrary fetch targets

## 6. OpenAPI and Contract Fixtures

- [ ] 6.1 Extend OpenAPI summary parameters and add typed facet path values, filter parameters, opaque cursor, bounded pages, examples, and standard error responses
- [ ] 6.2 Extend route-inventory and contract tests so every implemented operation, parameter, closed facet enum, page bound, evidence-link constraint, and correlated error is represented
- [ ] 6.3 Expand backend-owned fixtures with filtered summary, all four item details, sightings, groups, occurrences, every facet, pagination, empty and terminal pages, all release states, and invalid-cursor, unauthorized, not-found, and server-error envelopes
- [ ] 6.4 Add schema/shape tests for every fixture and unsafe inert values across all semantic and Kubernetes display fields

## 7. Frontend Synchronization Handoff

- [ ] 7.1 Inspect the frontend repository's existing snapshot and type-generation workflow read-only and record the exact synchronization, `schema.d.ts` generation, and clean-diff CI commands in the handoff
- [ ] 7.2 Record backend OpenAPI and fixture revision/checksum metadata and a checklist requiring the frontend snapshot to contain every inventory and facet operation before UI implementation
- [ ] 7.3 Verify formatting, Clippy, workspace tests, inventory PostgreSQL integration tests, OpenAPI tests, fixture checks, deployment-policy tests, strict OpenSpec validation, and archive ordering documentation
