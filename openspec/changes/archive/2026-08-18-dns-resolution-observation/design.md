## Context

`network.connect` observes the IP address after name resolution and intentionally performs no reverse DNS. Plaintext DNS traffic contains the name-to-address evidence operators need, but names are sensitive, DNS may be cached or encrypted, responses may contain multiple names and addresses, and packet capture can accidentally broaden the privacy boundary. The design must preserve the existing opt-in workload scope, bounded eBPF/userspace resources, typed protocol, tenant isolation, exact IP-based connection grouping, and safe Web rendering.

## Goals / Non-Goals

**Goals:**

- Observe bounded plaintext DNS queries and responses for selected Kubernetes workloads over UDP and TCP port 53.
- Emit canonical typed DNS events and correlate recent address answers to connection occurrences with explicit confidence and TTL.
- Preserve IP and port as connection-group identity while exposing DNS evidence as optional contextual metadata.
- Bound packet reads, parsing, state, cardinality, labels, names, answers, TTL, and retention; reject malformed data safely.
- Provide mixed-version rollout, metrics, API/UI investigation, acceptance, and rollback.

**Non-Goals:**

- Decrypting or intercepting DoH, DoT, DNSCrypt, TLS, HTTP, or application payloads.
- Claiming that a DNS name caused a connection, replacing IP identity with a name, or guaranteeing context when caches are used.
- Capturing arbitrary DNS resource data, EDNS option bodies, full packets, source ports, or reverse-resolving addresses.
- Building a general packet-capture product, recursive resolver, policy engine, or service map.

## Decisions

### Capture bounded DNS messages at the cgroup-aware packet boundary

Opt-in ingress and egress eBPF packet programs will inspect only IPv4/IPv6 UDP or TCP traffic where either port is 53, recover socket cgroup identity, and copy a fixed maximum prefix plus direction and tuple metadata to a ring buffer. All other traffic is rejected before payload access. The agent will attach programs only when DNS observation is enabled and will fail readiness if the required hooks or cgroup attribution helper are unavailable.

This is preferred to libc uprobes, which miss static/custom resolvers, and syscall buffer probes, which are difficult to associate reliably with peer port and scatter/gather layouts. TCP DNS framing and bounded reassembly remain userspace concerns; incomplete or oversized streams are counted and discarded.

### Parse DNS with strict canonical bounds

Userspace will validate the DNS header, QR bit, transaction ID, opcode, truncation, question count, compression pointers, label lengths, total canonical name length, record counts, RCODE, and wire bounds. It will retain only canonical lower-case ASCII/IDNA wire names, `A`/`AAAA` answers, bounded CNAME links, TTLs, and response code. Compression loops, unsupported classes/types, malformed packets, oversized names, fragmented data beyond the reassembly limit, and excess answers produce counters rather than partial events.

Names are normalized without reverse lookup. Raw packet bytes, TXT/MX/SRV data, EDNS options, source ephemeral ports, and unrelated DNS payload content are never serialized or logged.

### Model query and response as separate typed events

The domain and protobuf contracts will add `network.dns.query/v1` and `network.dns.response/v1`. Query events contain canonical query name/type and process/workload context. Response events contain the matching question, RCODE, bounded canonical address answers and CNAME chain, and effective TTL. Strict conversion rejects inconsistent query/response shapes before acknowledgement.

Separate events preserve evidence and failure semantics better than mutating a connect event after storage. They also allow queries with no response, NXDOMAIN, and responses with no address answer to remain inspectable.

### Use two bounded correlations with explicit confidence

The agent correlates query and response by workload/cgroup, transport, resolver endpoint, DNS transaction ID, question name/type, and a short timeout. It then maintains a TTL-clamped recent mapping from trusted workload scope plus canonical answer IP to one or more names. A subsequent `network.connect` occurrence may carry a bounded `dns_context` snapshot with observed names, observation time, expiry, and confidence `observed_recently`.

Correlation does not alter the connection fingerprint. Multiple valid names remain an ordered bounded set and are rendered as ambiguous evidence. Expired mappings, cache-only connections, unmatched responses, and encrypted DNS yield no context rather than guessed data.

### Persist immutable evidence and safe derived context

Raw typed DNS events use existing runtime-event storage, grouping, release summaries, occurrence APIs, and first-seen outbox guarantees. DNS groups use trusted scope, event kind, normalized process command, query name/type, and for responses the response code; individual answer sets remain occurrence evidence to limit unstable group identity.

Connection occurrences store the context observed at ingestion so later TTL expiry does not rewrite history. Semantic connection summaries remain IP-first and MUST NOT accumulate unbounded names. API/OpenAPI use closed typed schemas, and Web renders names as inert text with ambiguity/age labels.

### Keep observability and rollout cardinality-safe

Heartbeat and server metrics add bounded reason counters without name, address, tenant, workload, transaction ID, or event ID labels. Configuration defaults off and exposes limits for captured bytes, pending transactions, TCP streams, names per address, answers per response, TTL clamp, and events per second. Acceptance enables one fixture workload, checks cardinality and losses, and rolls back by disabling DNS probes first.

## Risks / Trade-offs

- **Encrypted DNS is opaque** → report an explicit unavailable limitation and never attempt decryption.
- **DNS cache creates connections without nearby queries** → omit context and explain absence; never reverse-resolve.
- **Shared/CDN IPs create ambiguous names** → retain a bounded set with `observed_recently` confidence and do not change grouping.
- **TCP segmentation or packet loss prevents parsing** → bounded reassembly with timeout/loss counters; emit no partial names.
- **Global packet hooks increase overhead** → early port/protocol rejection, fixed reads, opt-in attachment, rate limits, and staged rollout.
- **Domain names are sensitive and high-cardinality** → tenant scoping, bounded lengths/counts/TTL, no metric labels, inert UI, documented retention.
- **Cgroup attribution may be unavailable on some kernels/directions** → readiness failure for required support or counted unattributed discard; never guess workload ownership.

## Migration Plan

1. Ship additive domain/protobuf/OpenAPI support and server storage/API handling while agents keep DNS observation disabled.
2. Ship Web support for typed DNS events and optional connection context, including unavailable and ambiguity states.
3. Deploy the agent with probes disabled; verify capability compatibility and baseline counters.
4. Enable DNS observation for one controlled workload, run UDP/TCP, IPv4/IPv6, NXDOMAIN, malformed, cached, ambiguous, unselected, and encrypted-DNS fixtures, and inspect storage/API/UI/cardinality.
5. Expand selectors only after bounded counters and group growth remain acceptable.

Rollback disables `observation.network.dns` and rolls back the agent first. Additive server/Web versions and stored typed events remain readable; no data, Secret, or PVC deletion is required.

## Open Questions

- Whether the first production profile should require TCP DNS support on every supported kernel or advertise UDP and TCP as separate sub-capabilities.
- The default maximum effective TTL and whether tenant operators may lower, but never raise, the platform clamp.
- Whether DNS event retention should follow raw runtime-event retention or use a shorter privacy-specific maximum.
