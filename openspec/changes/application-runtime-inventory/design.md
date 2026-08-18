## Context

Okoscope currently persists every accepted runtime event, assigns it to one deterministic runtime group, and maintains release-scoped group summaries. Runtime-group identity deliberately includes cluster, namespace, and top-level workload identity so finding lifecycle and first-seen notifications remain local to a trusted deployment scope. That identity is correct for investigation, but it duplicates equivalent behavior when a user asks the application-level question, “What does this application do?”

The existing event model already contains the required process, destination, DNS, syscall, release, and Kubernetes evidence. The change therefore introduces a read model and APIs rather than new eBPF capture. The design must preserve tenant isolation, exact idempotent counts, bounded responses, stable semantic identity, raw-evidence navigation, and safe evolution as inventory identity changes.

## Goals / Non-Goals

**Goals:**

- Present one application-level inventory across contributing clusters, namespaces, and workloads.
- Define deterministic, versioned identities for process, destination, domain, and syscall items.
- Maintain exact first/last seen times and occurrence counts under retries, concurrency, and delayed events.
- Summarize bounded release and Kubernetes sightings while preserving navigation to runtime groups and raw occurrences.
- Expose tenant-safe, filterable, cursor-paginated APIs described by OpenAPI.
- Support restartable backfill and reconciliation for existing data.

**Non-Goals:**

- Changing runtime-group fingerprints, status lifecycle, notifications, or release-diff behavior.
- Adding eBPF event kinds or capturing additional sensitive data.
- Claiming causal relationships between processes, DNS, and connections.
- Adding risk scoring, approved baselines, environment entities, service maps, or generated enforcement policy.
- Treating lack of observation as proof that behavior cannot occur.

## Decisions

### 1. Inventory is a separate application-scoped projection

Inventory items use Organization, Project, and Application as trusted scope and intentionally omit cluster, namespace, workload, Pod, container, and release from item identity. Deployment-specific runtime groups remain unchanged and are linked to their application-level inventory item.

This preserves existing finding semantics while presenting equivalent behavior once. Replacing or weakening the runtime-group fingerprint was rejected because it would merge triage state across operational scopes, alter first-seen notifications, and invalidate existing release summaries.

### 2. Inventory identity is deterministic and versioned

Each item stores `inventory_kind`, `identity_version`, a canonical semantic identity, and a length-delimited cryptographic digest. The first identity version is:

| Kind | Canonical identity |
|---|---|
| `process` | executable |
| `syscall` | process command + canonical syscall name |
| `destination` | process command + address family + canonical destination IP + destination port |
| `domain` | process command + canonical DNS name + query type |

DNS query and response events intentionally map to the same domain item. Response code, answer addresses, CNAMEs, TTL, connection outcome, errno, parent command, release, and Kubernetes attribution remain occurrence evidence or bounded facets and do not change identity.

A changed canonicalization rule requires a new identity version and explicit backfill. Reusing runtime-group digests was rejected because their trusted deployment scope and DNS response identity differ from inventory semantics.

### 3. PostgreSQL projection tables maintain exact summaries

The server adds application-scoped inventory items plus idempotent links for raw events, runtime groups, releases, and deployment sightings. A representative shape is:

- `runtime_inventory_items`: identity, safe summary, first/last seen, occurrence count;
- `runtime_inventory_event_memberships`: one event-to-item membership per identity version;
- `runtime_inventory_group_links`: contributing runtime groups;
- `runtime_inventory_releases`: exact per-release first/last seen and occurrence count;
- `runtime_inventory_sightings`: bounded aggregation keys for cluster, namespace, workload, Pod UID, and container name.

Projection updates occur in the same ingestion transaction after raw-event persistence and grouping. Event membership uniqueness makes agent retries idempotent; row conflict handling and locking make concurrent updates exact; `LEAST` and `GREATEST` handle delayed events. The raw event and existing group membership remain the source evidence.

Computing every list response directly from JSON payloads and raw events was rejected as the production path because it requires repeated high-cardinality scans and JSON extraction. A separate analytics store was rejected as premature operational complexity.

### 4. Release presence expresses evidence, not enforcement certainty

An item is `observed` in a release when at least one trusted, release-attributed membership exists. It is `not_observed` only when the release has other trusted attributed runtime evidence in the selected scope and time window but none for the item. It is `unknown` when there is no trustworthy attributed evidence with which to evaluate the item.

The API returns the state and supporting counts/timestamps. It does not say `absent`, and the UI must not imply that `not_observed` proves impossibility. This gives useful release navigation now while leaving formal coverage confidence to a later capability.

### 5. Lists are compact; high-cardinality evidence is separately paginated

The inventory list returns semantic identity, aggregate lifecycle fields, and bounded counts such as release, cluster, namespace, workload, Pod, and container-name counts. It does not embed every release, Pod, container instance, group, or event.

Separate item endpoints paginate release presence, deployment sightings, contributing groups, and raw occurrences. Sightings aggregate by trusted deployment dimensions and expose Pod UID only in bounded detail responses; container name is a facet while ephemeral container IDs remain raw occurrence evidence.

### 6. API scope follows the Project/Application hierarchy

Routes live below `/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory`. Organization scope is always derived from the authenticated principal. List filtering supports kind, release, cluster, namespace, workload, container name, observation window, and bounded semantic search. Stable cursor ordering uses `(last_seen_at, id)` for the main list and explicit deterministic tuples for detail collections.

OpenAPI is updated with closed inventory-kind and release-presence enums, bounded page sizes, typed safe identities, and existing error/correlation conventions. Search applies only to allowlisted semantic fields and never to unrestricted raw JSON.

### 7. Backfill and reconciliation are explicit operational paths

A restartable bounded backfill derives memberships and summaries from existing raw events using a selected inventory identity version. It uses the same projection function as live ingestion, skips existing memberships, emits no first-seen webhook work, and records progress and reason counters.

An operator reconciliation check compares projection membership and aggregate counts against source records by bounded tenant/application partitions. Deployments may create tables, run backfill, and expose the API in separate rollout stages.

### 8. The Web UI consumes the typed inventory contract

The separately developed Web UI uses the summary endpoint for application-level counts, a shared paginated list component for the four inventory kinds, and an item detail view for release presence, deployment sightings, group links, and occurrences. Every page keeps the active cluster, namespace, workload, release, and observation-window scope visible. Observed semantic and Kubernetes strings are rendered as inert text using the same safety boundary as existing DNS evidence.

Because the frontend is maintained outside this repository, this change supplies the complete OpenAPI contract, representative fixtures, and a frontend handoff/acceptance document; coordinated frontend delivery is required before the capability is considered product-complete.

## Risks / Trade-offs

- **Projection write amplification and storage growth** → Keep one compact membership per raw event, use aggregate tables and targeted indexes, benchmark realistic cardinality, and retain bounded operational metrics.
- **Projection drift after partial failures or future bugs** → Update transactionally during ingestion, provide idempotent backfill and reconciliation, and expose projection freshness and mismatch metrics.
- **Application-wide aggregation hides deployment differences** → Return scope counts, make cluster/namespace/workload filters first-class, and preserve links to the original scoped runtime groups.
- **`not_observed` may be mistaken for guaranteed absence** → Use evidence-specific terminology, return supporting evidence metadata, reserve `unknown`, and document that inventory is observational.
- **DNS query and response merging loses response-code identity at list level** → Preserve response codes and answers in occurrences and bounded item detail summaries while keeping the user-facing domain identity stable.
- **Search or facets can create expensive queries** → Restrict searchable fields, require bounded page sizes and observation windows where needed, add projection-specific indexes, and verify query plans at representative scale.
- **A future identity version can temporarily duplicate items** → Require callers and backfill to select an active identity version and never silently merge versions.

## Migration Plan

1. Add projection tables, constraints, and indexes without changing existing tables or APIs.
2. Deploy live transactional projection updates behind an inventory-readiness flag and verify idempotency metrics.
3. Run bounded restartable backfill per tenant/application and identity version with external notifications suppressed.
4. Reconcile projection counts and time bounds against source memberships; mark an application ready only after successful reconciliation.
5. Enable inventory read APIs and Web UI for ready applications while continuing live projection updates.
6. Roll back by disabling inventory reads and live projection updates; existing events, groups, releases, and notifications remain valid. Projection tables may be retained for diagnosis and safely rebuilt later.

## Open Questions

- What production cardinality and retention targets should drive the first index and partition strategy?
- Should release `not_observed` require only some attributed release evidence, or a future explicit minimum coverage threshold before it is shown outside an experimental label?
- Should the first UI expose all clusters by default or require an initial cluster/namespace scope when one Application spans staging and production?
