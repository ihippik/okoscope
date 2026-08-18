## Why

Okoscope stores application runtime behavior as workload-scoped findings and raw occurrences, but users cannot see a single application-level inventory of processes, outbound destinations, DNS domains, and syscalls. A unified inventory is needed to answer what an application does, where and when a behavior was observed, and in which releases it was present without manually reconciling duplicate runtime groups across clusters, namespaces, and workloads.

## What Changes

- Add an application-scoped runtime inventory that derives stable semantic items from existing runtime groups without changing runtime-group fingerprint or lifecycle semantics.
- Cover four inventory kinds: executed processes, outbound destinations, DNS domains, and syscalls.
- Summarize each item with first and last observation time, exact occurrence count, and bounded counts or facets for releases, clusters, namespaces, workloads, Pods, and containers.
- Add tenant-safe, cursor-paginated APIs for inventory summaries, item listing, item detail, release presence, and deployment-scope sightings.
- Support bounded filtering by kind, release, cluster, namespace, workload, container, observation window, and semantic search.
- Preserve evidence navigation from every inventory item to its contributing runtime groups and raw occurrences.
- Distinguish behavior that was observed in a release, not observed in available release evidence, and unknown because trustworthy attributed evidence is unavailable.
- Keep process-to-DNS-to-connection causality, risk scoring, baselines, environment modeling, policy generation, and new eBPF event types outside this change.

## Capabilities

### New Capabilities

- `application-runtime-inventory`: Defines application-level semantic inventory identity, aggregation, release presence, deployment sightings, evidence navigation, filtering, pagination, tenant isolation, and operational correctness.

### Modified Capabilities

None. Existing runtime-event grouping, release diff, event capture, and raw occurrence contracts remain unchanged and serve as source evidence for the new capability.

## Impact

- Adds versioned authenticated Web API routes and OpenAPI schemas under the existing Project/Application hierarchy.
- Adds server-side inventory query/projection logic and likely PostgreSQL projection tables, indexes, and an idempotent backfill path.
- Reads existing `runtime_event_groups`, `runtime_event_group_memberships`, `runtime_event_group_releases`, `runtime_events`, and `releases` data without changing their identity or lifecycle contracts.
- Requires Web UI integration for an application inventory overview, four kind views, filtering, item detail, and links to existing runtime-group evidence.
- Adds metrics, structured logs, tests, and operator verification for projection freshness, reconciliation, pagination, tenant isolation, and bounded query behavior.
