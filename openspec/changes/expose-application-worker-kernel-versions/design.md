## Context

The agent already reads `/proc/sys/kernel/osrelease` and sends `architecture` and `kernel_release` in `AgentHello`. Session registration currently discards both values, while the `agents` table otherwise represents the current state of a worker identified by `(cluster_id, node_name)`. Runtime events retain `agent_id` and `application_id`, so they provide server-derived evidence that a worker has observed a particular application.

The Web UI is deployed separately and consumes the OpenAPI-described HTTP API. The change therefore crosses protocol ingestion, PostgreSQL state, application-scoped querying, HTTP conventions, and the published contract. Existing agents and existing database rows must remain usable during rolling upgrades.

## Goals / Non-Goals

**Goals:**

- Retain the latest platform metadata reported by every agent-backed worker.
- Expose all distinct workers with runtime evidence for an owned application, including heterogeneous kernel releases.
- Distinguish missing platform metadata from a literal reported value.
- Preserve tenant isolation, bounded queries, deterministic cursor pagination, and the existing API error conventions.
- Keep agent/server rolling upgrades backward compatible.

**Non-Goals:**

- Maintaining a history of kernel upgrades or attributing every historical event to an immutable platform snapshot.
- Inferring Linux distribution, kernel support status, vulnerability state, or eBPF compatibility from a release string.
- Treating the application as having one canonical kernel version.
- Changing the protobuf wire shape or adding platform data to heartbeats.
- Implementing the separately deployed frontend.

## Decisions

### Store current platform state on `agents`

Add nullable `architecture` and `kernel_release` columns to `agents`. Session registration writes normalized hello values on insert and refreshes them on conflict together with agent version and capabilities. Empty, whitespace-only, and the agent's current `unknown` sentinel are stored as SQL null.

This matches the existing meaning of `agents` as current worker state and requires no new wire fields. Storing the values only on `agent_sessions` would preserve connection snapshots but make the common current-state query more complex. Copying them to every runtime event would create unnecessary duplication and falsely imply immutable event-time accuracy.

### Derive application membership from accepted runtime events

The application worker collection groups owned `runtime_events` by `agent_id` and joins the scoped `agents` and `clusters` rows. The grouped minimum and maximum event timestamps become `first_observed_at` and `last_observed_at`; `agents.last_seen_at` is returned separately as `agent_last_seen_at`.

This avoids a new mutable application-worker association table and ensures membership is based on durable server-accepted evidence. It also means an agent with no accepted event for an application is not listed, even if it is connected to the same cluster.

### Provide a dedicated bounded collection endpoint

Expose `GET /api/v1/projects/{project_id}/applications/{application_id}/workers`. The response item contains agent ID, cluster ID and name, node name, agent version, nullable architecture and kernel release, application observation bounds, and agent last-seen time. Results sort by `(last_observed_at DESC, agent_id DESC)` and use an opaque cursor over that tuple. Existing default and maximum page-limit conventions apply.

A dedicated endpoint keeps application detail bounded and avoids overloading runtime-inventory concepts: a worker is observation infrastructure, not an application runtime behavior identity. Returning a single application-level kernel value is rejected because simultaneous workers can legitimately disagree.

### Keep activity interpretation on explicit timestamps

The API returns observation and heartbeat timestamps but does not define an `online` boolean. A boolean would require a server-wide staleness threshold and could become incorrect in cached UI state. The frontend can render recency using an agreed presentation threshold without losing the underlying evidence.

### Apply existing tenant-safe ownership checks

The handler derives organization scope from authentication, verifies that the application belongs to the project in the path, and constrains the worker query by organization, project, and application. A mismatched or foreign resource returns the existing tenant-safe not-found envelope. Platform strings are bounded at ingestion or response validation and rendered as inert text by clients.

## Risks / Trade-offs

- **Current state overwrites kernel history** → Document the endpoint as current reported worker metadata; add session snapshots in a future change only if historical attribution becomes a concrete requirement.
- **Platform data remains null until an agent reconnects after migration** → Keep columns and API fields nullable and make unavailable state explicit in fixtures and frontend guidance.
- **Application worker aggregation can scan many runtime events** → Use a grouped, scoped query, inspect its plan against production-shaped data, and add a supporting index only when the plan demonstrates it is required.
- **Agent reconnect is the refresh boundary** → Document this semantics; no heartbeat protocol expansion is needed for kernel data that normally remains stable for a boot.
- **Untrusted node-provided strings reach the browser** → Bound and normalize values, describe concrete OpenAPI lengths, and require inert text rendering.

## Migration Plan

1. Add a forward-only migration with nullable platform columns and update every required-migration pin.
2. Deploy the migrated server; old rows and old agents continue with null metadata.
3. Deploy agents normally. Each reconnect populates or refreshes the worker's platform fields.
4. Publish the endpoint and OpenAPI schema; the frontend handles null until workers reconnect.

Rollback of application code is safe because older server binaries ignore the additive nullable columns. The migration need not drop them during rollback.

## Open Questions

- Whether a later frontend milestone needs a server-defined activity status and threshold rather than raw timestamps.
- Whether future investigations require immutable kernel snapshots per session or event; that is intentionally deferred until history is needed.
