## Why

Outbound connection events currently expose only the destination IP and port because `connect()` receives an already-resolved address. Operators need privacy-bounded evidence of the DNS names observed by selected workloads so they can understand which external services an IP connection may represent without relying on misleading reverse DNS.

## What Changes

- Add opt-in observation of plaintext DNS queries and responses over UDP and TCP port 53 for selected workloads.
- Introduce typed `network.dns.query` and `network.dns.response` events with bounded names, record types, response codes, canonical answer addresses, process identity, and trusted Kubernetes attribution.
- Correlate responses to queries and recent DNS answers to `network.connect` occurrences using bounded, TTL-based state and an explicit confidence model; retain destination IP as canonical connection identity.
- Exclude packet payloads beyond the bounded DNS fields, EDNS option contents, non-address resource data, and encrypted DoH/DoT contents; document cache, CNAME, shared-IP, and ambiguity limitations.
- Extend protocol capabilities, loss counters, storage, grouping, APIs, OpenAPI, notifications, and Web UI with safe typed DNS information and no domain-valued metric labels.
- Provide controlled IPv4/IPv6, cache, ambiguity, malformed-packet, unselected-workload, and encrypted-DNS acceptance coverage with a documented rollback.

## Capabilities

### New Capabilities

- `dns-resolution-observation`: Privacy-bounded DNS query/response capture, validation, correlation, investigation, limitations, and deployed acceptance.

### Modified Capabilities

- `workload-event-observation`: Add strict opt-in DNS configuration, bounded packet parsing and correlation state, and DNS-specific loss accounting.
- `agent-server-session`: Add versioned typed DNS transport capabilities and heartbeat counters while preserving mixed-version compatibility.
- `runtime-event-storage`: Validate and durably store canonical DNS events and optional DNS context on connection occurrences.
- `runtime-event-grouping`: Define DNS group identity and bounded correlated-name context without making names part of connection-group identity.
- `outbound-network-observation`: Enrich connection investigation with explicitly qualified recent DNS evidence while retaining IP-first semantics.
- `web-ui-api-foundation`: Expose and render safe DNS evidence, ambiguity, unavailable states, and encrypted-DNS limitations without clickable untrusted names.

## Impact

The change affects the eBPF shared layout and probes, agent configuration/decoding/correlation/counters, protobuf protocol, server ingestion/storage/grouping/metrics, runtime-group and occurrence APIs, OpenAPI-generated Web types, observability UI and tests, Kubernetes examples, privacy documentation, and acceptance fixtures. It adds no destructive migration and remains disabled by default; existing `network.connect`, process, and syscall behavior remains compatible when DNS observation is disabled.
