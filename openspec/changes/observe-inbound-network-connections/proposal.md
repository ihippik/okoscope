## Why

Okoscope can show where a workload connects outbound, but it cannot show which TCP endpoints the workload exposes or which inbound connections the application actually accepts. Adding bounded listener and accepted-connection evidence lets users discover unexpected ports, investigate reachability, and compare the application's inbound behavior across releases without capturing payloads.

## What Changes

- Add opt-in, capability-reported observation of successful TCP listening endpoints as typed `network.listen` events for IPv4 and IPv6 selected workloads.
- Add opt-in, capability-reported observation of successfully accepted TCP connections as typed `network.accept` events containing local and remote socket endpoints without packet or application payloads.
- Use kernel socket observation that reports the effective local address and port, including kernel-assigned ports, while retaining trustworthy cgroup, process, and Kubernetes attribution.
- Extend the agent protocol, validation, loss counters, durable event storage, deterministic grouping, raw occurrence APIs, and operational metrics for both event kinds.
- Group listener behavior by process and local endpoint; group accepted connections by the receiving process and local endpoint while retaining remote endpoints as bounded occurrence evidence rather than unbounded group identity.
- Include listener behavior in release comparison so users can identify new, disappeared, and unchanged exposed endpoints.
- Integrate listeners and accepted-connection evidence with the application runtime inventory after the existing inventory capability is materialized, without creating one inventory item per remote client address.
- Keep UDP binding, rejected/SYN-only attempts, packet payloads, HTTP/TLS inspection, and remote Kubernetes identity enrichment out of this change.

## Capabilities

### New Capabilities

- `inbound-network-observation`: Defines safe, bounded TCP listener and accepted-connection capture, attribution, event semantics, privacy boundaries, and capability degradation.

### Modified Capabilities

- `workload-event-observation`: Adds strict opt-in configuration and supported-capability behavior for inbound TCP observation.
- `agent-server-session`: Adds backward-compatible typed transport and loss reporting for `network.listen/v1` and `network.accept/v1`.
- `runtime-event-storage`: Adds canonical validation and durable storage requirements for local and remote socket endpoints.
- `runtime-event-grouping`: Adds deterministic grouping and safe summaries that prevent remote-client cardinality from creating unbounded groups.
- `release-runtime-diff`: Adds listener endpoint behavior to release comparisons.
- `application-runtime-inventory`: Adds application-scoped inbound endpoint inventory and bounded accepted-connection evidence after the completed inventory change is materialized.

## Impact

- Extends the eBPF/userspace ABI, agent observer attachment, configuration, capability advertisement, heartbeat counters, event model, and protobuf schema.
- Extends server ingestion validation, PostgreSQL payload storage, grouping and inventory projection, release comparison, API/OpenAPI schemas, fixtures, metrics, and integration tests.
- Requires compatibility testing on the documented Linux kernel range because socket tracepoint context and attribution must remain trustworthy; unsupported kernels advertise neither inbound capability and must not claim complete observation.
- Increases event volume for busy servers, so accepted connections share existing global bounds and require dedicated opt-in rate limiting and loss visibility.
- Logically depends on the completed `application-runtime-inventory` change being archived or otherwise materialized before this change is archived.
