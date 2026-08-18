## Context

The completed `application-runtime-inventory` change exposes application-level inventory summary, list, detail, release presence, sightings, runtime groups, and occurrences. Review from the separately maintained Web UI identified four contract gaps before implementation: summary cards cannot follow active scope filters, deployment filter values are not discoverable, returned evidence links need an explicit trust boundary, and the frontend fixture omits several documented states.

The hardening change is additive to the runtime inventory capability and must not change inventory fingerprints, projection counts, ingestion, or evidence storage. The frontend repository has its own OpenAPI snapshot and generated `schema.d.ts`; backend OpenAPI remains the source of truth, but this repository cannot silently mutate or verify files outside its planning scope.

## Goals / Non-Goals

**Goals:**

- Make summary cards describe exactly the active release, deployment, time, and semantic-search scope.
- Provide scalable bounded option discovery for deployment facets.
- Specify stable dependent-facet semantics and tenant-safe cursor pagination.
- Ensure evidence navigation cannot become an arbitrary server-directed fetch primitive.
- Supply complete contract fixtures for frontend states and safe rendering tests.
- Make frontend OpenAPI snapshot synchronization and type regeneration an explicit handoff gate.

**Non-Goals:**

- Changing application inventory identity, projection tables, release-presence semantics, or raw evidence.
- Adding environment modeling, baselines, risk scoring, causal relationships, or policy generation.
- Replacing the existing Releases API with an inventory-specific release facet.
- Implementing the Web UI in this repository.
- Returning every facet value in one unbounded response.

## Decisions

### 1. Summary uses the list's operational filters but remains cross-kind

`GET .../runtime-inventory/summary` accepts `release_id`, `cluster_id`, `namespace`, `workload_kind`, `workload_name`, `container_name`, `observed_from`, `observed_to`, and `search` with the same validation and matching semantics as the inventory list. It does not accept `kind`; its purpose is to return all four kind cards within the shared active scope.

The query is built from one common normalized filter model shared with list and facets so identical inputs cannot drift semantically. Returning only global Application totals was rejected because it makes cards contradict the filtered list. Returning summary inside every list response was rejected because it couples pagination with aggregate cost and duplicates data across pages.

### 2. Facets are separate bounded collections

The API adds:

```text
GET .../runtime-inventory/facets/{facet}
```

Supported facets are `cluster`, `namespace`, `workload_kind`, `workload_name`, and `container_name`. Responses contain a stable opaque value, display label, matching inventory-item count, matching occurrence count, and an opaque cursor. Cluster values are UUID strings with the trusted cluster name as label; textual facets use the canonical stored value as both value and label.

Facet requests accept `kind`, release, deployment, observation-window, semantic search, facet-option search, cursor, and limit. Release choices remain on the Releases API because releases are first-class resources with established ordering and metadata.

A single response containing every facet was rejected because distinct values can be high-cardinality and each dimension needs independent search and pagination. Free-text-only filters were rejected as the default UX because they are error-prone and require users to know exact observed values.

### 3. Requested facet ignores its own selected value

Facet counts apply every active filter except the filter for the dimension being requested. For example, namespace options respect release, cluster, workload, container, time, search, and kind filters but ignore the currently selected namespace. This lets a user replace or broaden one selection without first clearing it.

All other selected dimensions remain effective. Counts are exact distinct inventory-item and accepted-occurrence aggregates within that resulting scope. Ordering is deterministic by matching item count descending, display label ascending, and stable value ascending; the opaque cursor encodes the full ordering tuple.

### 4. Facet queries use the existing projection and targeted indexes

Facet and scoped-summary queries join `runtime_inventory_items`, `runtime_inventory_sightings`, and `runtime_inventory_releases`. Implementation first verifies representative `EXPLAIN (ANALYZE, BUFFERS)` plans. A new additive index is introduced only if the existing tenant/Application, release, and sighting indexes cannot meet documented bounds.

Precomputed facet tables were rejected for this milestone because they add transactional write amplification and reconciliation surfaces before real cardinality demonstrates need.

### 5. Evidence navigation is route-derived and relative

The canonical frontend behavior is to construct detail collection paths from the known Project, Application, item, and endpoint suffixes in the typed OpenAPI contract. Existing `evidence` link strings may remain as convenience hints to avoid a breaking response change, but the server MUST generate only root-relative paths matching one of the four allowlisted routes for that exact scope:

- `releases`;
- `sightings`;
- `groups`;
- `occurrences`.

Links cannot contain a scheme, authority, query, fragment, path traversal, encoded separator, or a different scoped identifier. Contract tests validate emitted links. The frontend handoff instructs clients not to fetch arbitrary returned strings.

Removing the links immediately was rejected as an unnecessary breaking contract change. Treating arbitrary backend URLs as trusted was rejected because observed or compromised data must not redirect authenticated API requests.

### 6. Fixtures mirror complete typed responses and failures

Backend-owned JSON fixtures add complete examples for item detail, sightings, groups, occurrences, all cursor-bearing pages, filtered summary, each facet, and standard correlated error envelopes for invalid cursor, unauthorized, not found, and server failure. Unsafe examples cover every semantic and Kubernetes display field while remaining schema-valid.

Contract tests deserialize fixtures against their intended typed schema or validate their required shape, maximum page sizes, closed identity objects, link policy, and error structure. Local frontend builders may extend UI-only combinations but cannot replace backend contract fixtures as the integration source of truth.

### 7. Frontend snapshot synchronization is a release gate

After backend OpenAPI and fixtures pass, the handoff records the exact source file revision and requires the frontend repository to:

1. replace its local OpenAPI snapshot from the reviewed backend artifact;
2. regenerate `schema.d.ts` using its repository-owned command;
3. fail CI if regeneration leaves a diff or required inventory operations/types are missing;
4. run typed fixture tests before runtime inventory UI implementation begins.

The backend change cannot mark the external repository work complete; it provides a verifiable handoff checklist and checksums or revision metadata. Frontend mutation happens in a subsequent frontend-scoped change.

## Risks / Trade-offs

- **Scoped summary and facets add aggregate query cost** → Share normalized predicates, use projection tables only, benchmark representative cardinality, retain 200-item limits, and add indexes only from measured plans.
- **Dependent facet semantics can surprise users** → Document the “all filters except self” rule in OpenAPI and handoff and test every dimension.
- **Exact occurrence counts across sightings can double-count an item spanning Pods** → Item counts use distinct item IDs while occurrence counts aggregate event memberships or a deduplicated item scope, never sum overlapping joins blindly.
- **Opaque cursors become invalid after scope changes** → Scope cursors to facet plus normalized filter fingerprint and return a stable invalid-cursor error when reused elsewhere.
- **Evidence hints duplicate client route construction** → Keep them only for compatibility, enforce allowlisted relative paths, and describe route construction as canonical.
- **Backend and frontend snapshots can drift again** → Record synchronization as a release gate and add frontend CI drift detection in the frontend-scoped change.
- **Two active changes target one capability** → Archive completed `application-runtime-inventory` before archiving this hardening delta; validate both changes before implementation handoff.

## Migration Plan

1. Archive or otherwise materialize the completed `application-runtime-inventory` capability before this hardening change is archived.
2. Add shared filter normalization, scoped-summary behavior, and facet routes behind the existing authenticated API boundary.
3. Update OpenAPI, contract fixtures, tests, query benchmarks, and operational metrics.
4. Deploy the additive backend contract; existing list/detail clients remain compatible.
5. Hand the reviewed OpenAPI snapshot and fixtures to the frontend repository, regenerate its types, and verify snapshot drift CI before UI implementation.
6. Roll back by deploying the prior server: facet routes and scoped summary filters disappear, while inventory identity, data, and existing unfiltered endpoints remain intact.

## Open Questions

- Which frontend repository command currently generates `schema.d.ts`, and does it already enforce a clean-diff check in CI?
- What production upper bounds for distinct namespaces, workloads, and container names should be included in the first facet benchmark dataset?
