## 1. Inventory Data Model and Identity

- [x] 1.1 Add a PostgreSQL migration for versioned inventory items, event memberships, runtime-group links, release summaries, deployment sightings, constraints, and tenant-scoped indexes
- [x] 1.2 Implement canonical length-delimited inventory fingerprinting for process, destination, unified domain, and syscall identities with safe semantic summaries
- [x] 1.3 Add unit tests proving identity stability, scope separation, DNS query/response unification, excluded volatile fields, invalid field handling, and identity-version isolation
- [x] 1.4 Add migration tests for fresh databases, upgrades from the current schema, foreign-key tenant consistency, uniqueness, and bounded lookup indexes

## 2. Live Projection

- [x] 2.1 Implement the shared idempotent projection operation that creates event membership and updates item first/last seen and exact occurrence count
- [x] 2.2 Extend the projection operation to maintain contributing runtime-group links and exact release summaries
- [x] 2.3 Extend the projection operation to maintain cluster, namespace, workload, Pod UID, and container-name sightings and distinct list counts
- [x] 2.4 Invoke inventory projection in the existing event ingestion transaction after persistence and grouping without changing runtime-group or notification behavior
- [x] 2.5 Add concurrency, retry, delayed-event, cross-scope aggregation, and transaction-rollback integration tests for live projection

## 3. Backfill, Reconciliation, and Operations

- [x] 3.1 Add a bounded restartable inventory backfill command using the live projection operation and an explicit identity version
- [x] 3.2 Persist or expose backfill progress and ensure resumed or repeated batches skip existing event memberships without emitting first-seen notifications
- [x] 3.3 Add a bounded per-Application reconciliation operation for source membership, aggregate count, and first/last observation mismatches
- [x] 3.4 Add low-cardinality metrics and structured logs for live projection outcomes, backfill progress and skip reasons, projection freshness, reconciliation mismatches, and query latency
- [x] 3.5 Add operator queries and documentation for staged rollout, readiness verification, reconciliation, rollback, and safe projection rebuild

## 4. Inventory Read APIs

- [x] 4.1 Add authenticated Project/Application inventory summary and item-list routes with stable cursor pagination and bounded page sizes
- [x] 4.2 Implement allowlisted filters for kind, release, cluster, namespace, workload, container name, observation window, and bounded semantic search
- [x] 4.3 Add item detail and separately paginated release-presence, deployment-sighting, contributing-group, and raw-occurrence routes
- [x] 4.4 Implement evidence-qualified `observed`, `not_observed`, and `unknown` release classification with exact supporting fields
- [x] 4.5 Add API tests for all four kinds, filters, cursor stability, high-cardinality response bounds, evidence navigation, invalid inputs, and consistent errors
- [x] 4.6 Add tenant-isolation tests covering cross-Organization Project, Application, release, item, group, and occurrence references without existence disclosure

## 5. OpenAPI and Frontend Contract

- [x] 5.1 Extend `openapi/okoscope-v1.yaml` with typed inventory routes, safe kind-specific identities, release-presence enums, filters, pagination, errors, and representative examples
- [x] 5.2 Extend OpenAPI contract tests and verify every implemented inventory route and response is represented by the specification
- [x] 5.3 Add bounded frontend fixtures covering each inventory kind, multi-scope aggregation, release states, pagination, empty state, and inert markup-like observed values
- [x] 5.4 Write the separate-Web-UI handoff and acceptance document for overview counts, four inventory views, visible active scope, filters, item detail, evidence navigation, and safe rendering

## 6. End-to-End Verification

- [x] 6.1 Add an end-to-end fixture that observes equivalent behavior across multiple Pods and deployment scopes and verifies one application inventory item with distinct sightings
- [x] 6.2 Verify release attribution and all three release-presence states against controlled attributed and unattributed evidence
- [x] 6.3 Benchmark inventory projection and representative list/detail queries at documented event, item, release, and Pod cardinalities and record accepted limits
- [x] 6.4 Run formatting, linting, unit, integration, migration, OpenAPI, deployment-policy, and documentation verification required by the repository
