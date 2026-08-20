## ADDED Requirements

### Requirement: Durable canonical inbound network storage
The server SHALL validate and durably store typed `network.listen` and `network.accept` payloads using TCP transport, matching canonical address families, non-zero local ports, bounded endpoint fields, and the existing trusted tenant, workload, process, release, observation-time, transaction, replay, and acknowledgement guarantees.

#### Scenario: Valid listener event is committed
- **WHEN** a valid attributed `network.listen` event is accepted
- **THEN** its effective local endpoint is stored transactionally and the batch is acknowledged only after commit

#### Scenario: Valid accepted connection is committed
- **WHEN** a valid attributed `network.accept` event is accepted
- **THEN** its immutable local and remote endpoints are stored as raw occurrence evidence under the authenticated agent scope

#### Scenario: Inbound event is replayed
- **WHEN** the same agent-scoped inbound event ID is delivered again
- **THEN** raw storage, grouping, release summaries, inventory projection, and first-seen work remain idempotent

#### Scenario: Endpoint validation fails
- **WHEN** an inbound event contains a zero local port, unsupported family or transport, mismatched address encoding, or an out-of-bounds field
- **THEN** the server rejects the containing batch before acknowledgement and stores no partial event
