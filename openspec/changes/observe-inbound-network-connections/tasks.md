## 1. Kernel Hook and Attribution Spike

- [x] 1.1 Inventory the supported kernel baseline and candidate TCP socket hooks for listener and server-side accepted transitions.
- [ ] 1.2 Build a bounded spike proving effective IPv4/IPv6 local endpoints, remote accept endpoints, current process/cgroup attribution, wildcard sockets, port-zero assignment, and null userspace peer-address behavior.
- [x] 1.3 Record the selected hook ABI, transition predicates, capability checks, unsupported-kernel behavior, and kernel-matrix evidence in the design or implementation notes before production capture work.

## 2. Event Model and Wire Contract

- [x] 2.1 Add validated canonical `NetworkListen` and `NetworkAccept` event-model types, endpoint invariants, event kind names, and unit tests.
- [x] 2.2 Extend the protobuf schema additively with typed listener/accept payloads, independent capability strings, and inbound loss counters without renumbering existing fields.
- [x] 2.3 Implement protocol encode/decode and malformed-payload rejection tests for IPv4, IPv6, zero ports, family mismatches, unknown enums, and older-participant compatibility.
- [x] 2.4 Update checked-in/generated protocol artifacts and verify regeneration leaves no unexpected diff.

## 3. eBPF Capture and Userspace Decoding

- [x] 3.1 Extend the fixed kernel/userspace ABI with aligned inbound event records, compile-time size assertions, explicit event kinds, and bounded counters.
- [x] 3.2 Implement verified TCP listener transition capture with effective local address/port and safe process/cgroup context.
- [x] 3.3 Implement verified server-side accepted-connection capture with canonical local/remote endpoints and safe process/cgroup context.
- [ ] 3.4 Add eBPF predicate, address decoding, wildcard, dual-stack, transition deduplication, malformed context, and ring-capacity tests or fixtures supported by the kernel harness.
- [x] 3.5 Extend userspace kernel record decoding and validation with unit tests for both event kinds and every loss reason.

## 4. Agent Configuration, Capabilities, and Delivery

- [x] 4.1 Add strict default-disabled listener and accept configuration, a bounded accepted-event rate limit, validation, example configuration, and config tests.
- [ ] 4.2 Attach inbound hooks independently, fail readiness for configured unavailable capabilities, and advertise `network.listen/v1` and `network.accept/v1` only after verified attachment.
- [x] 4.3 Resolve cgroup/container/workload attribution for inbound candidates, apply workload selectors, and construct typed runtime events without endpoint logging.
- [x] 4.4 Integrate listener events with global buffering/rate limits and accept events with dedicated plus global limits while preserving replay and batching behavior.
- [x] 4.5 Extend heartbeat snapshots, protobuf conversion, logs, and agent metrics with additive unlabeled inbound loss counters and tests.

## 5. Server Ingestion and Durable Storage

- [x] 5.1 Add ingestion validation for inbound transport, family, endpoint, port, process, capability, tenant, workload, release, and observation-time invariants.
- [x] 5.2 Persist canonical inbound payloads through existing transaction and event-ID idempotency paths, adding a migration or indexes only where the current JSON storage/query plan requires them.
- [ ] 5.3 Add ingestion integration tests for valid listener/accept events, duplicate delivery, mixed invalid batches, cross-tenant attribution, optional release resolution, and correlated error behavior.
- [x] 5.4 Add bounded accepted/listener operational metrics without address, port, process, or workload label cardinality.

## 6. Runtime Grouping and Occurrence APIs

- [x] 6.1 Implement versioned listener fingerprints and safe semantic summaries keyed by trusted scope, process, transport, family, and local endpoint.
- [x] 6.2 Implement accepted-connection fingerprints keyed by the receiving local endpoint while retaining remote endpoints only in immutable occurrences.
- [x] 6.3 Extend first-seen outbox materialization and notification-safe summaries so remote endpoints never enter default group or notification content.
- [ ] 6.4 Add grouping concurrency, retry, delayed-event, release-summary, different-client/same-listener, different-local-endpoint, and bounded occurrence API tests.

## 7. Release Comparison and Application Inventory

- [ ] 7.1 Materialize or archive the completed `application-runtime-inventory` capability before applying overlapping inventory projection and specification changes.
- [ ] 7.2 Extend release runtime diff classification and fixtures for new, disappeared, unchanged, and unknown listener behavior while excluding accept traffic variation from identity.
- [x] 7.3 Add inbound endpoint inventory identity and projection from listener and accept evidence without remote-client identity or cardinality.
- [x] 7.4 Preserve distinguishable `listener_observed` and `accept_observed` evidence across release presence, deployment sightings, group links, occurrences, backfill, and reconciliation.
- [ ] 7.5 Extend scoped summary, list/detail, facets where applicable, pagination, unsafe-value fixtures, query benchmarks, and reconciliation tests for inbound endpoints.

## 8. API Contract, Verification, and Rollout

- [x] 8.1 Update OpenAPI event, group, release-diff, inventory, capability, counter, and standard error schemas with closed inbound types and bounded fields.
- [ ] 8.2 Add complete backend contract fixtures and API tests for listener/accept group summaries, raw occurrences, inventory evidence states, pagination, malformed values, and inert endpoint rendering.
- [x] 8.3 Update operator documentation with privacy boundaries, configuration, capability/readiness behavior, loss interpretation, PostgreSQL verification queries, and canary procedures.
- [ ] 8.4 Run formatting, linting, unit, integration, OpenAPI, migration, eBPF build, kernel-matrix, and representative event-rate/storage benchmarks.
- [ ] 8.5 Deploy server compatibility first, canary listener observation, then rate-limited accept observation; record rollback checks and production loss/cardinality acceptance thresholds.
