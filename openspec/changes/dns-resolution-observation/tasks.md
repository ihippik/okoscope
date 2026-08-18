## 1. Domain and Protocol Contracts

- [x] 1.1 Add strict canonical DNS name, query type, transport, direction, response code, address answer, CNAME, TTL, confidence, ambiguity, and qualified context domain types with boundary tests.
- [x] 1.2 Add typed `network.dns.query` and `network.dns.response` runtime payloads plus optional immutable DNS context on `network.connect` occurrences without changing connection identity.
- [x] 1.3 Extend protobuf messages, capabilities, conversions, unknown-field compatibility, and malformed name/answer/TTL/context rejection.
- [x] 1.4 Add additive heartbeat counters for packet decode, malformed compression, truncation, unsupported records, correlation miss/capacity, TCP reassembly, rate/capacity, and kernel output loss.

## 2. Bounded eBPF DNS Capture

- [x] 2.1 Define fixed-layout cgroup-aware DNS packet records with direction, transport, tuple, sequence/framing metadata, bounded bytes, and compile-time size/alignment assertions.
- [x] 2.2 Attach opt-in ingress and egress packet programs that reject non-IPv4/IPv6 and non-port-53 traffic before bounded payload access and preserve trusted cgroup identity.
- [x] 2.3 Parse only bounded IP/UDP/TCP framing in kernel space, safely handle IPv4 options and IPv6 extension/fragment cases, and send candidate DNS prefixes through the existing bounded ring buffer.
- [x] 2.4 Add kernel-side counters for unsupported framing, attribution failure, decode failure, oversize input, and ring-buffer reservation failure without destination or name labels.
- [x] 2.5 Add Linux build/verifier and packet-layout tests for IPv4/IPv6, UDP/TCP, ingress/egress, fragmentation, truncation, malformed lengths, non-DNS traffic, and probe cleanup.

## 3. Agent DNS Parsing and Correlation

- [x] 3.1 Add strict default-disabled DNS configuration with bounded byte, transaction, stream, answer, name, TTL, and event-rate limits plus examples and unknown-field tests.
- [x] 3.2 Implement a bounded DNS wire parser covering questions, compression pointers, `A`/`AAAA`, bounded CNAME chains, RCODE and truncation while excluding unsupported record and EDNS contents.
- [x] 3.3 Implement bounded timeout-based TCP DNS framing/reassembly and discard incomplete, overlapping, oversized, or expired streams with reason counters.
- [x] 3.4 Correlate queries and responses by trusted workload/cgroup, transport, resolver, transaction ID, canonical question and timeout, including NXDOMAIN and unmatched-response behavior.
- [x] 3.5 Maintain a workload-scoped TTL-clamped address-to-name cache and attach immutable `observed_recently` context to exact subsequent connection IPs with bounded ambiguity.
- [x] 3.6 Integrate DNS events and context with attribution, filtering, rate limiting, buffering, batching, replay, status logs, and heartbeats without name/address-valued labels.
- [x] 3.7 Add parser property/fuzz fixtures and agent tests for compression loops, case normalization, IDNA wire names, multiple answers, CNAMEs, cache expiry, ambiguity, encrypted DNS absence, overload, and privacy exclusions.

## 4. Server Storage and Grouping

- [x] 4.1 Validate DNS payload and qualified context invariants before batch acknowledgement, including tenant/workload ownership and bounded canonical fields.
- [x] 4.2 Persist and retrieve DNS events and immutable connection context through raw storage and occurrence APIs with replay/idempotency and mixed-version integration tests.
- [x] 4.3 Add deterministic DNS query/response fingerprints over trusted scope, process, question/type and response code while excluding transaction IDs and volatile answer sets.
- [x] 4.4 Preserve IP-first connection fingerprints when DNS contexts differ and expose only bounded occurrence-specific context in semantic summaries and first-seen outbox payloads.
- [x] 4.5 Add concurrent grouping, release-summary/diff, lifecycle, notification, CNAME/answer variation, ambiguity, expiry, and first-seen deduplication tests.

## 5. API, OpenAPI, and Operational Safety

- [x] 5.1 Extend API serialization and closed OpenAPI schemas/examples for DNS summaries, occurrences, and qualified connection context with all forbidden fields excluded.
- [x] 5.2 Verify event-kind filtering, cursor pagination, tenant-safe not-found behavior, request IDs, lifecycle commands, release diff, and notifications through Web API integration tests.
- [x] 5.3 Add bounded metrics and structured logs for capture, parsing, correlation, ingestion, grouping, ambiguity, and loss without domain, IP, tenant, workload, transaction, PID, or event labels.
- [x] 5.4 Document configuration, kernel/platform requirements, plaintext-only boundary, cache/CNAME/shared-IP ambiguity, DoH/DoT limitations, retention/cardinality, troubleshooting, enablement, and rollback.
- [x] 5.5 Publish a frontend handoff covering generated types, inert names, confidence/ambiguity/age, unavailable states, filters, release diff, notifications, accessibility, and forbidden content.

## 6. Web UI Integration

- [x] 6.1 Sync OpenAPI into Okoscope Web, regenerate typed clients, and add compile-time fixtures for query, response, connection context, ambiguity, and unavailable states.
- [x] 6.2 Render safe DNS group summaries, occurrences, release diffs, notifications, and qualified connection context as inert text without reverse lookup or automatic navigation.
- [x] 6.3 Add component tests for UDP/TCP, A/AAAA, CNAME, NXDOMAIN, multiple answers/names, TTL/age, ambiguity, null release, large counts, encrypted/cache absence, and forbidden fields.
- [x] 6.4 Add Playwright and accessibility coverage for filtering DNS behavior and following DNS evidence into a connection investigation with correlated API errors and credential-safe state.

## 7. End-to-End Acceptance and Release

- [x] 7.1 Add controlled selected/unselected fixtures for UDP and TCP DNS, IPv4/IPv6 answers, success/NXDOMAIN, CNAME, shared-IP ambiguity, cached connections, malformed packets, encrypted DNS, and replay.
- [x] 7.2 Trace fixture evidence through eBPF, agent parsing/correlation, PostgreSQL, grouping, release summaries, first-seen notification, API, and Web UI while asserting forbidden and unselected data is absent.
- [x] 7.3 Run Rust formatting/lint/unit/integration/fuzz checks, Linux eBPF build/verifier tests, OpenAPI validation, frontend quality gates, container smoke tests, and Kubernetes manifest validation.
- [x] 7.4 Publish immutable server, agent, and Web images; roll out server/Web first, enable DNS only for the fixture, record counters/cardinality/privacy/smoke evidence, and restore DNS-disabled configuration.
