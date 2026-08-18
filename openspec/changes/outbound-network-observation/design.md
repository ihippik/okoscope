## Context

The agent currently attaches tracepoints for process execution and allowlisted syscall entry, emits a fixed kernel record through a ring buffer, resolves the current cgroup to trusted Kubernetes workload configuration in userspace, and transports typed events over the long-lived gRPC session. The server stores payload JSON, computes deterministic versioned fingerprints, exposes runtime groups and occurrences, compares release-scoped summaries, and emits first-seen notifications.

Outbound `connect()` observation crosses all of those boundaries. Unlike the existing syscall signal, its useful arguments are a userspace `sockaddr` available at syscall entry while its result is available only at syscall exit. The supported production profile is Linux x86_64 with cgroup v2; collection must remain payload-free, opt-in, bounded, and compatible with agents that do not advertise the new capability.

## Goals / Non-Goals

**Goals:**

- Observe completed outbound IPv4 and IPv6 connect syscall attempts for configured workloads.
- Preserve destination, process, workload, release, and result semantics end to end.
- Bound kernel memory, ring-buffer pressure, userspace queues, storage, grouping cardinality, and metrics.
- Reuse existing ingestion, deduplication, grouping, lifecycle, release diff, notification, and investigation paths.
- Make privacy boundaries and ambiguous non-blocking results explicit and testable.

**Non-Goals:**

- Packet capture, flow byte counts, source ports, HTTP/TLS parsing, DNS observation, or payload inspection.
- Proving that a TCP handshake completed or tracking socket lifetime after the syscall returns.
- Inferring TCP versus UDP from the file descriptor in the first version.
- Inbound accept, bind, listen, Kubernetes Service enrichment, network topology graphs, policy enforcement, blocking, or risk scoring.
- Unix-domain, netlink, packet, Bluetooth, or other socket families.

## Decisions

### Correlate syscall-specific entry and exit tracepoints

Attach `syscalls/sys_enter_connect` and `syscalls/sys_exit_connect` only when `observation.network.connect` is enabled. At entry, copy only the family-specific bounded `sockaddr_in` or `sockaddr_in6` fields into a fixed pending value keyed by `pid_tgid`, together with entry timestamp, cgroup ID, and command. At exit, remove the pending value, classify the signed return code, and emit one fixed-size completed record.

This is preferred over the existing generic syscall allowlist because a generic event lacks destination and completion result. A cgroup connect hook was considered, but it observes the pre-connect decision point and does not directly provide the syscall completion result required by the product semantics. Socket-state probes were rejected because they would expand scope into transport-specific lifecycle tracking.

### Treat events as attempts, not established connections

The domain payload uses address family, canonical destination IP, destination port, and outcome. Return zero maps to `succeeded`, `-EINPROGRESS` maps to `in_progress`, and other negative results map to `failed` with positive errno. No event claims that a TCP handshake completed, and no transport protocol is inferred.

This preserves kernel truth for blocking and non-blocking applications. Collapsing `EINPROGRESS` into success would be misleading; treating it as failure would generate false alarms.

### Use fixed bounded kernel structures and explicit loss counters

Use a fixed-capacity hash map for pending entries and fixed-size address storage rather than heap allocation or variable-length copies. Reject invalid lengths before reading user memory, support only `AF_INET` and `AF_INET6`, remove matched entries on exit, and count map insertion failure, missing correlation, decode failure, unsupported family, and ring-buffer reservation failure in a small kernel counter map read by userspace.

A regular bounded map is preferred over silent LRU eviction because loss must be observable and must never correlate an exit with unrelated state. The capacity becomes a documented constant for the first release; configuration remains limited to enabling the capability so unsafe arbitrary kernel allocation is impossible.

### Extend the typed payload additively

Add a protobuf `NetworkConnect` oneof variant and additive heartbeat counter fields without changing the protocol version. Agents advertise `network.connect/v1`; older agents and servers continue using existing variants. The transport decoder validates address byte length by family, port range, result/errno consistency, and UTF-8-free bounded primitives before constructing the domain payload.

The domain model serializes canonical textual IP addresses so stored JSON, API occurrences, webhook semantic metadata, and Web rendering share a stable representation. The wire protocol uses fixed 4-byte or 16-byte addresses to avoid ambiguous input strings.

### Reuse storage and introduce network-specific fingerprint semantics

The existing runtime event JSON payload is sufficient; no destination columns are required for the initial bounded group and occurrence queries. Fingerprinting adds a new match arm and encodes normalized process command, address family, canonical address bytes, and big-endian port after the existing trusted scope and event kind. Outcome and errno remain occurrence facts and do not split endpoint identity.

Exact destination IP grouping is intentionally chosen over CIDR bucketing because first-seen behavior and release diffs need stable concrete evidence. Cardinality is controlled by opt-in workload selection, the existing global event-rate limit and queue bounds, exact event deduplication, and one group per process/destination tuple. Service identity enrichment can later add display metadata without changing the version-one fingerprint.

### Present bounded semantic data through existing investigation surfaces

Runtime group list/detail, occurrence pagination, event-kind filtering, release diff, lifecycle actions, and first-seen notification flow remain the primary surfaces. Network summaries display process command, address family, destination IP, and destination port; occurrence detail additionally displays outcome and errno. The OpenAPI examples and frontend typed rendering must not introduce packet bodies, reverse DNS, clickable unescaped URLs, or destination-derived metric labels.

No dedicated network endpoint or topology API is added in this change. This keeps authorization, pagination, stale/error behavior, and request correlation aligned with existing runtime investigation.

## Risks / Trade-offs

- [High-cardinality destinations, especially rotating external IPs] → Keep observation opt-in, apply existing workload/rate/queue bounds, expose drop metrics, document cardinality, and defer Service/CIDR aggregation until evidence supports it.
- [Non-blocking connect does not reveal eventual handshake result] → Model `in_progress` explicitly and describe all records as syscall attempts.
- [PID exits before userspace attribution] → Carry entry cgroup and command in the completed kernel record, resolve immediately, count unattributed events, and never guess workload ownership.
- [Tracepoint layout varies by kernel or architecture] → Limit support to the documented platform profile, validate required tracepoints and expected context decoding at startup/acceptance, and remain unready when the enabled capability cannot attach safely.
- [Pending entries survive when an exit is not observed] → Bound the map, expose capacity pressure, and replace an existing same-key entry deterministically; process-level PID reuse cannot leak destination data across emitted events because only exit-time removal emits.
- [Destination IP can itself be sensitive infrastructure metadata] → Tenant-scope every read, exclude it from logs and metric labels, avoid reverse DNS, and retain the same authorization as raw occurrences.
- [Protocol additions encounter mixed versions] → Gate emission on local configuration and advertise an explicit capability while retaining additive protobuf compatibility and rejection tests.

## Migration Plan

1. Add domain, protobuf, validation, grouping, and storage support while network capture remains disabled by default.
2. Add eBPF correlation, userspace decoding, counters, strict configuration, and capability reporting.
3. Add API/OpenAPI examples, frontend rendering, tests, documentation, and a controlled acceptance fixture.
4. Publish immutable server, agent, and Web images; migrate the server first if implementation discovers a schema migration, then roll out the compatible server and Web before enabling agents.
5. Enable `observation.network.connect` for one bounded fixture workload, verify counters and cardinality, then expand selectors deliberately.

Rollback disables `observation.network.connect` and rolls back the agent first. Additive protocol and stored JSON remain readable by the compatible server; existing runtime data is preserved and no reverse migration or deletion is required.

## Open Questions

- What fixed pending-map capacity provides sufficient burst tolerance on the documented cluster without excessive locked memory? Resolve through an eBPF load test before enabling production workloads.
- Should a later capability enrich exact destination IPs with Kubernetes Service identity at observation time or presentation time? This change preserves exact evidence and defers enrichment.
