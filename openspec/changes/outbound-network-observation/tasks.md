## 1. Domain and Protocol Contract

- [x] 1.1 Add canonical address-family and connect-outcome domain types plus the typed `NetworkConnect` event payload with strict constructors and serialization tests.
- [x] 1.2 Extend `RuntimeEvent::kind`, protocol protobuf messages, wire conversions, and malformed payload rejection for `network.connect` while preserving compatibility with existing variants.
- [x] 1.3 Add `network.connect/v1` capability reporting and additive heartbeat counters for correlation capacity, correlation misses, decode failures, unsupported families, and kernel output loss.
- [x] 1.4 Add protocol round-trip, unknown-field compatibility, invalid address/port/outcome/errno, and mixed-version tests.

## 2. Bounded eBPF Capture

- [x] 2.1 Define fixed-layout shared pending-connect and completed-connect kernel records for IPv4 and IPv6 with compile-time size and alignment assertions.
- [x] 2.2 Attach opt-in `sys_enter_connect` capture that validates length and family, copies only bounded destination fields, and inserts entry context into a fixed-capacity map keyed by `pid_tgid`.
- [x] 2.3 Attach `sys_exit_connect` capture that removes matching state, classifies zero, `EINPROGRESS`, and failed results, and emits one completed ring-buffer record.
- [x] 2.4 Add kernel-side bounded counters for insertion failure, missing correlation, address decode failure, unsupported family, and ring-buffer reservation failure.
- [x] 2.5 Add eBPF/userspace decoder tests for byte order, IPv4, IPv6, malformed lengths, unsupported families, signed errno handling, record layout, and correlation cleanup.

## 3. Agent Configuration and Processing

- [x] 3.1 Add strict default-disabled `observation.network.connect` configuration, validation, example configuration, and unknown-field tests.
- [x] 3.2 Load and attach connect probes only when enabled, fail readiness safely when required tracepoints cannot attach, and leave existing process/syscall observation unchanged when disabled.
- [x] 3.3 Convert completed kernel records into canonical typed runtime events using entry cgroup/process context and existing trusted workload attribution.
- [x] 3.4 Integrate network events with existing rate limiting, buffering, batching, replay, drop accounting, status logging, and heartbeats without destination-valued labels.
- [x] 3.5 Add agent tests proving selected/unselected workload behavior, capability negotiation, successful/failed/in-progress outcomes, unattributed events, overload, and privacy exclusions.

## 4. Server Ingestion, Storage, and Grouping

- [x] 4.1 Validate network payload invariants during protocol conversion and ingestion before batch acknowledgement, including tenant and workload ownership.
- [x] 4.2 Persist and retrieve canonical network payloads through existing raw event storage and occurrence APIs with ingestion replay/idempotency integration tests.
- [x] 4.3 Add deterministic version-one network fingerprinting over trusted scope, process command, family, exact destination, and port while excluding outcome and errno.
- [x] 4.4 Add safe semantic summaries, first-seen outbox payloads, webhook metadata, lifecycle behavior, and release-scoped summaries for network groups.
- [x] 4.5 Add grouping and release-diff tests for repeated mixed outcomes, changed address/port/process, replay, concurrent ingestion, first-seen deduplication, and release comparison.

## 5. API, OpenAPI, and Operations

- [x] 5.1 Extend API serialization and OpenAPI schemas/examples so network group summaries and occurrences expose only canonical destination, process, outcome, and bounded errno fields.
- [x] 5.2 Verify event-kind filtering, cursor pagination, tenant-safe not-found behavior, request IDs, lifecycle commands, and notification delivery for network groups through Web API integration tests.
- [x] 5.3 Add bounded metrics and structured logs for capture, ingestion, grouping, and loss outcomes without destination, tenant, workload, PID, or event identifiers as metric labels.
- [x] 5.4 Document configuration, kernel/platform requirements, privacy boundaries, `EINPROGRESS` semantics, cardinality trade-offs, troubleshooting counters, enablement, and rollback.
- [x] 5.5 Publish a frontend handoff with generated-client requirements, safe network summary/occurrence rendering, filters, release diff behavior, error states, accessibility, and forbidden fields.

## 6. Web UI Integration

- [x] 6.1 Sync the accepted OpenAPI contract into Okoscope Web, regenerate typed clients, and add compile-time fixtures for network summary and occurrence payloads.
- [x] 6.2 Render safe `network.connect` summaries in runtime-group lists, detail, occurrences, release diffs, and first-seen notification context without reverse DNS or clickable untrusted destinations.
- [x] 6.3 Add unit/component tests for IPv4, IPv6, success, in-progress, failure/errno, large counts, null release attribution, and forbidden payload fields.
- [x] 6.4 Add Playwright and accessibility coverage for filtering and investigating a network group with correlated API errors and credential-safe state.

## 7. End-to-End Acceptance and Release

- [x] 7.1 Add a controlled acceptance fixture that produces selected IPv4 success, deterministic failure, IPv6 behavior where available, unsupported-family traffic, duplicate replay, and unselected-workload traffic.
- [ ] 7.2 Trace accepted fixture events through eBPF, agent session, PostgreSQL, grouping, release summaries, first-seen notification, API, and Web UI while asserting excluded data is absent.
- [ ] 7.3 Run Rust formatting/lint/unit/integration checks, eBPF build and platform tests, OpenAPI validation, frontend quality gates, container smoke tests, and Kubernetes manifest validation.
- [ ] 7.4 Publish immutable server, agent, and Web images; roll out compatible server/Web first, enable network observation only for the fixture workload, and record rollout, counters, cardinality, smoke, and rollback evidence.
