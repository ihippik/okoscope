## ADDED Requirements

### Requirement: Deterministic outbound destination grouping
The server SHALL group `network.connect` events using the trusted tenant and workload scope, fingerprint version, event kind, normalized process command, address family, canonical destination IP, and destination port; connection outcome and errno MUST NOT change group identity.

#### Scenario: Repeated outcomes target the same endpoint
- **WHEN** the same selected workload and process command attempts the same destination with successful, in-progress, and failed outcomes
- **THEN** all idempotently accepted occurrences update one runtime group while retaining their individual outcomes

#### Scenario: Destination changes
- **WHEN** the destination address or port differs for an otherwise identical network attempt
- **THEN** the server assigns the event to a distinct deterministic runtime group

#### Scenario: Release attribution changes
- **WHEN** identical network behavior occurs in two resolved Application releases
- **THEN** group identity remains stable and exact release-scoped summaries support existing new, disappeared, and unchanged comparison

### Requirement: Safe network semantic summary
Each network group SHALL expose a bounded semantic summary containing process command, address family, canonical destination IP, and destination port, and first-seen notification materialization SHALL use only those safe fields plus existing group identity and tenant-scoped metadata.

#### Scenario: First network behavior is grouped
- **WHEN** the first accepted occurrence creates a `network.connect` group
- **THEN** exactly one first-seen outbox record is created with the safe semantic summary and no packet, DNS, HTTP, TLS, source-port, or socket-buffer content
