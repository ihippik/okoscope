## ADDED Requirements

### Requirement: Versioned inbound network event transport
The agent protocol SHALL add backward-compatible typed `network.listen` and `network.accept` payloads with independent `network.listen/v1` and `network.accept/v1` capabilities, and the server MUST validate transport, family, canonical endpoint, port, and payload invariants before acknowledging a batch.

#### Scenario: Compatible listener event is delivered
- **WHEN** an accepted agent advertising `network.listen/v1` sends a valid listener event
- **THEN** the server processes it using existing transaction, acknowledgement, replay, and event-ID deduplication guarantees

#### Scenario: Compatible accepted-connection event is delivered
- **WHEN** an accepted agent advertising `network.accept/v1` sends a valid accepted-connection event
- **THEN** the server validates and processes its local and remote endpoints without interpreting any endpoint as tenant identity

#### Scenario: Inbound payload is malformed
- **WHEN** an inbound payload has an unsupported transport or family, malformed address, zero local port, or inconsistent endpoint fields
- **THEN** the server rejects the batch before partial acknowledgement without logging raw payload bytes

#### Scenario: Older participant receives additive fields
- **WHEN** an older compatible agent or server encounters additive inbound variants, capabilities, or counters it does not understand
- **THEN** existing session and non-inbound event behavior remains compatible and no unknown payload becomes trusted evidence

### Requirement: Inbound observation loss is reported safely
Agent heartbeats SHALL report additive monotonic counters for inbound decode, attribution, unsupported-family, kernel-output, rate-limit, and applicable correlation losses without endpoint, process, or workload labels.

#### Scenario: Inbound evidence is dropped
- **WHEN** a bounded inbound capture stage rejects or drops a candidate event
- **THEN** the next heartbeat exposes the matching reason counter without including the candidate endpoint
