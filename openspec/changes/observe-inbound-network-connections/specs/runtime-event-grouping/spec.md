## ADDED Requirements

### Requirement: Deterministic listener grouping
The server SHALL group `network.listen` events using trusted tenant and workload scope, fingerprint version, event kind, normalized process command, TCP transport, address family, canonical local address, and local port; backlog and other occurrence-specific fields MUST NOT change group identity.

#### Scenario: Listener is observed repeatedly
- **WHEN** the same selected workload and process repeatedly listens on the same effective local endpoint
- **THEN** all idempotently accepted occurrences update one `network.listen` group

#### Scenario: Listener endpoint changes
- **WHEN** the local address, local port, process command, or trusted workload scope differs
- **THEN** the server assigns the listener evidence to a distinct deterministic group

### Requirement: Accepted connections group by receiving endpoint
The server SHALL group `network.accept` events using trusted tenant and workload scope, fingerprint version, event kind, normalized receiving process command, TCP transport, address family, canonical local address, and local port; remote address and remote port MUST remain occurrence evidence and MUST NOT change group identity.

#### Scenario: Many clients use one listener
- **WHEN** different remote endpoints are accepted by the same selected workload process on one local endpoint
- **THEN** the occurrences update one `network.accept` group while retaining their individual remote endpoint evidence

#### Scenario: Receiving endpoint differs
- **WHEN** otherwise equivalent accepted connections use another local address or local port
- **THEN** the server assigns them to a distinct group

### Requirement: Inbound summaries are safe and bounded
Inbound group summaries and first-seen notification materialization SHALL expose the process and canonical local endpoint, MAY expose bounded aggregate remote counts, and MUST NOT expose remote endpoints, packet contents, or unbounded client-derived values outside authorized occurrence detail.

#### Scenario: First accepted connection creates a group
- **WHEN** the first accepted occurrence creates a `network.accept` group
- **THEN** exactly one first-seen outbox record uses only safe local-endpoint and trusted group metadata

#### Scenario: Operator investigates accepted occurrences
- **WHEN** an authorized operator requests a bounded occurrence page for an inbound group
- **THEN** original accepted-connection events may include their immutable remote endpoints under existing tenant-scoped access controls
