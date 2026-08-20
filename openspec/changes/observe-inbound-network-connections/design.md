## Context

The agent currently attaches tracepoints for process execution, allowlisted syscalls, and outbound `connect`, plus cgroup packet programs for bounded DNS observation. The event pipeline already provides cgroup-to-container resolution, Kubernetes workload attribution, bounded buffering, capability advertisement, protobuf transport, raw PostgreSQL storage, deterministic runtime grouping, release summaries, and application inventory projection.

Inbound observation has two distinct semantics. A listening socket describes an endpoint exposed by the workload even when it receives no traffic. An accepted connection proves that the application accepted one TCP connection; a SYN or ingress packet alone does not. Generic `sys_enter_listen` exposes only a file descriptor and backlog, while `bind` is not equivalent to listening and may request port zero. The implementation therefore needs socket-state data that contains the effective endpoint after the kernel completes the transition.

Accepted connections can be high volume and remote addresses are potentially sensitive and high-cardinality. The design must retain investigation evidence without generating one durable group or inventory item per client address, and it must preserve the existing rule that no packet or application payload is captured.

## Goals / Non-Goals

**Goals:**

- Emit typed, attributed `network.listen` evidence for successful IPv4/IPv6 TCP listeners using the effective local endpoint.
- Emit typed, attributed `network.accept` evidence only for TCP connections accepted by a selected workload.
- Preserve process, Kubernetes, release, tenant, replay, grouping, occurrence, and inventory guarantees of existing events.
- Keep all kernel state, queues, rates, response pages, labels, and metrics bounded.
- Make unsupported or incomplete capture explicit through capabilities and reason counters.
- Let users compare exposed listener behavior across releases and investigate bounded accepted-connection evidence.

**Non-Goals:**

- UDP bind observation, SYN/rejected-attempt detection, packet accounting, flow duration, or connection close events.
- Packet payload, HTTP, TLS, credentials, Unix-domain sockets, or unrestricted socket metadata.
- Treating Kubernetes declared container ports or Services as proof that a process is listening.
- Resolving remote IP addresses to Kubernetes workloads, identities, DNS names, or geolocation in this change.
- Creating runtime groups or inventory items keyed by remote client address or ephemeral source port.

## Decisions

### 1. Listener and accepted connection are separate event kinds

`network.listen` contains transport, address family, canonical local address, non-zero effective local port, and a bounded backlog value when reliably available. `network.accept` contains transport, address family, canonical local and remote addresses, non-zero local port, and remote port. Both reuse the common process and trusted Kubernetes attribution envelope.

A single generic inbound event was rejected because listener state and accepted traffic have different lifecycles, volume, privacy, and grouping semantics. Calling packet arrival an accepted connection was rejected because it overstates what the application did.

### 2. Socket lifecycle hooks are primary; bind/listen file-descriptor correlation is not authoritative

The agent uses a documented kernel socket tracepoint or equivalent stable CO-RE-compatible hook that exposes TCP state transitions and effective endpoints. A listener event is emitted on a successful transition to `TCP_LISTEN`; an accept event is emitted only at a hook that proves a server-side socket became accepted/established and provides trustworthy current cgroup/process context.

The implementation must prove hook availability and attribution behavior over the supported kernel matrix before advertising a capability. Correlating `bind(fd)` with `listen(fd)` was rejected as the primary design because descriptors may be duplicated, inherited, passed between processes, or bound to port zero. `accept4` syscall-only capture was rejected as authoritative because applications may pass a null peer-address pointer.

If no supported hook can provide both semantic proof and trustworthy attribution on a kernel, the corresponding capability remains unavailable; the agent does not infer events from ingress packets. A short implementation spike precedes ABI changes to select and fixture-test the exact hook.

### 3. Observation is independently opt-in and capability reported

Configuration adds strict booleans `observation.network.listen` and `observation.network.accept`, both defaulting to false. Enabling one attaches only its required hooks. Hello advertises `network.listen/v1` and `network.accept/v1` independently only after successful attachment and capability verification.

This follows existing outbound network behavior and allows users to enable low-volume listener inventory without accepting the potentially much higher accepted-connection volume.

### 4. Kernel/userspace records and counters remain fixed and bounded

The fixed ABI gains explicit event kinds and endpoint fields, or a separate fixed inbound record if that produces safer alignment and decoding. Userspace validates family/address consistency, canonicalizes addresses, rejects zero local ports, and refuses unknown transports or malformed endpoint combinations.

Dedicated monotonic counters cover unsupported family, decode failure, attribution failure, ring loss, rate limiting, and any bounded correlation capacity/miss introduced by the selected hook. Counters never use addresses, ports, workloads, or processes as metric labels. Accepted events use a dedicated configured rate limit in addition to the existing global queue and event-rate limits; listener events use the global bounds.

### 5. Grouping identity describes server behavior, not clients

`network.listen` group identity is trusted workload scope, fingerprint version, event kind, normalized process command, transport, address family, canonical local address, and local port. Backlog is occurrence evidence and does not change identity.

`network.accept` uses the same local-endpoint identity plus its distinct event kind. Remote address and remote port remain immutable raw occurrence evidence but do not affect group identity. Safe summaries expose the local endpoint, aggregate occurrence count, and bounded distinct-remote counts; APIs reveal actual remote endpoints only through bounded, authorized occurrence pages.

Remote-address grouping was rejected because public endpoints and health checks would create unbounded groups, first-seen notifications, and inventory items. Dropping remote endpoints entirely was rejected because they are valuable investigation evidence already no more sensitive than outbound destination addresses.

### 6. Inventory models inbound endpoints and keeps accepts as evidence

Application inventory adds an inbound endpoint kind keyed by transport, family, canonical local address, and local port, independent of cluster, workload, Pod, container, and release. Listener and accept groups may contribute evidence to the endpoint, but remote clients never enter inventory identity.

An accepted connection without a retained listener occurrence may still establish observed endpoint evidence, because observation can begin after process startup or listener capture can be lost. The UI distinguishes `listener_observed` from `accept_observed` evidence rather than claiming a listener event existed.

### 7. Release comparison is observational and listener focused

Release diff includes `network.listen` groups using existing observed/not-observed/unknown evidence semantics. Accepted connection volume is excluded from behavioral identity comparisons because traffic changes independently from application behavior. Counts may remain available as evidence but do not determine whether an endpoint is new or disappeared.

### 8. Protocol evolution is additive

Protobuf adds new payload variants, endpoint messages, capability strings, and loss counters without renumbering existing fields or raising the protocol version solely for additive compatible fields. Older agents omit the new capabilities; a server validates each received payload independently and rejects malformed batches before acknowledgement.

## Risks / Trade-offs

- **Socket tracepoint context may not identify the accepting task on every supported kernel** → Run a kernel-matrix spike and advertise the capability only where semantic proof and cgroup attribution are verified.
- **Duplicate state transitions may emit duplicate logical observations** → Select exact transition predicates and include kernel fixture/unit tests; distinct accepted sockets remain distinct occurrences while retry deduplication remains event-ID based.
- **Busy listeners may overwhelm the pipeline** → Make accept capture independently opt-in, apply dedicated and global rate limits, expose losses, and aggregate UI behavior by local endpoint.
- **Wildcard listeners can appear as both IPv4 and IPv6 depending on kernel socket options** → Preserve observed kernel family/address exactly and do not infer dual-stack equivalence.
- **Remote addresses are sensitive evidence** → Keep them out of labels, notifications, group identity, inventory identity, and default list summaries; expose only in authorized bounded occurrence detail.
- **Observation starts after a long-lived listener was created** → Document that capture is observational; optionally evaluate a bounded startup socket snapshot in a later change rather than pretending event history is complete.
- **The application inventory capability is not yet materialized in main specs** → Archive/materialize its completed change before archiving this delta and sequence implementation to avoid conflicting projection work.

## Migration Plan

1. Run a bounded implementation spike against the supported kernel matrix and record the chosen hooks, tracepoint layouts, and attribution proof.
2. Add additive event-model, fixed ABI, protobuf, configuration, capabilities, and decoding support while both options remain disabled by default.
3. Deploy server support and migrations before enabling new agent options; older agents and existing event kinds remain compatible.
4. Enable listener observation for a canary workload, verify endpoint accuracy, counters, grouping, release summaries, and inventory projections.
5. Enable accepted-connection observation with a conservative rate limit, verify loss rates and storage/query cardinality, then expand selectively.
6. Roll back by disabling the two observation options or deploying the prior agent. The newer server can retain already accepted additive events; reads remain bounded and existing events are unaffected.

## Open Questions

- Which exact socket hook meets the supported kernel baseline while preserving trustworthy accepting-process and cgroup context?
- Should `backlog` be included when the chosen hook cannot report it consistently, or omitted from v1 instead of being nullable?
- What default and maximum accepted-connection event rates are safe for representative production workloads?
- Should wildcard endpoints (`0.0.0.0` and `::`) receive an explicit presentation classification, or remain canonical addresses interpreted by the UI?
