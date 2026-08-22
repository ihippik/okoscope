## Why

The Web UI cannot show which Linux kernels currently back an application, even though agents already report their node architecture and kernel release during session establishment. Operators need this per-worker visibility because one application can run across nodes with different kernel versions, which materially affects eBPF compatibility and runtime investigation.

## What Changes

- Persist each agent worker's current Linux kernel release and architecture when its session is registered.
- Expose a tenant-scoped, bounded application worker API derived from workers that have supplied runtime evidence for the application.
- Return worker identity, cluster context, agent version, architecture, kernel release, and first/last observation timestamps so the Web UI can render heterogeneous and unavailable values honestly.
- Document the endpoint and its concrete schemas in the OpenAPI contract.
- Preserve compatibility with existing rows and older agents by representing unavailable platform metadata as null.

## Capabilities

### New Capabilities
- `application-worker-platform-observability`: Defines how authenticated clients discover the workers observed for an application and inspect their current Linux kernel and architecture metadata.

### Modified Capabilities
- `agent-server-session`: Agent session registration will durably retain the platform metadata already carried by the hello message and refresh it on reconnect.
- `web-ui-api-foundation`: The published browser API contract will include a typed, bounded application worker collection endpoint.

## Impact

- Agent session registration and agent persistence in `crates/server/src/session.rs`.
- PostgreSQL `agents` schema, required migration version, and migration verification tests.
- A server HTTP route and tenant/project/application-scoped query joining application runtime evidence to agents and clusters.
- `openapi/okoscope-v1.yaml`, route coverage checks, API tests, fixtures, and frontend handoff documentation.
- No protocol wire change is required because `AgentHello` already contains `architecture` and `kernel_release`.
