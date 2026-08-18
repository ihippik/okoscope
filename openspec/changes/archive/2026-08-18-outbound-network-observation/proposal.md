## Why

Okoscope can identify process execution and selected syscalls, but it cannot show which external endpoints a workload attempts to reach. Outbound connection observation adds a high-value behavioral signal for investigating unexpected dependencies and release regressions without collecting packet payloads or application-layer secrets.

## What Changes

- Add opt-in observation of outbound IPv4 and IPv6 `connect()` attempts for selected Kubernetes workloads.
- Emit a typed `network.connect` event containing the destination address and port, process identity, Kubernetes attribution, observation time, and the completed syscall result.
- Correlate syscall entry and exit safely with bounded eBPF state, explicit overflow accounting, and support only for `AF_INET` and `AF_INET6` destinations.
- Extend the versioned agent protocol, capability negotiation, event storage, deterministic grouping, summaries, notifications, APIs, and Web UI investigation views for network events.
- Keep collection disabled by default and apply existing workload selection, buffering, batching, deduplication, rate limiting, and tenant isolation.
- Explicitly exclude packet payloads, HTTP data, TLS contents, DNS names, Unix sockets, source ephemeral ports, and unrestricted socket buffers.
- Provide automated unit, integration, eBPF, API, UI, and deployed acceptance coverage for successful, failed, filtered, unsupported, and overloaded observation paths.

## Capabilities

### New Capabilities

- `outbound-network-observation`: Opt-in capture, transport, investigation, and safe presentation of outbound connection attempts and outcomes.

### Modified Capabilities

- `workload-event-observation`: Add strict network observation configuration, supported address families, safety limits, and privacy exclusions.
- `agent-server-session`: Negotiate and transport typed `network.connect` events while reporting bounded correlation and capacity losses.
- `runtime-event-storage`: Validate and durably persist network event fields with the existing trusted tenant and workload attribution.
- `runtime-event-grouping`: Define deterministic network fingerprints, semantic summaries, release comparison behavior, and first-seen notification input.

## Impact

- eBPF programs and shared kernel/userspace event structures gain connect entry/exit correlation and destination decoding.
- Agent configuration, capability reporting, counters, buffering, and event conversion gain opt-in network support.
- The event model and protobuf protocol gain a backward-compatible typed payload variant and capability version.
- Server ingestion, validation, grouping, notification metadata, OpenAPI examples, and investigation responses gain network semantics; PostgreSQL continues storing typed payload JSON unless implementation validation identifies a required index or column.
- Okoscope Web gains safe network summaries and occurrence detail rendering without displaying unrestricted response or packet data.
- Kubernetes acceptance requires a controlled IPv4/IPv6 receiver or deterministic failed destination and verification that unselected workloads and unsupported socket families are not emitted.
