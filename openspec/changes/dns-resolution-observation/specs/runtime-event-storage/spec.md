## ADDED Requirements

### Requirement: Durable canonical DNS storage
The server SHALL validate and durably store typed DNS query/response evidence and immutable qualified connection DNS context with existing tenant, workload, release, observation-time, transaction, replay, and acknowledgement guarantees.

#### Scenario: Valid DNS evidence is committed
- **WHEN** a valid attributed DNS event is accepted
- **THEN** its bounded canonical fields are stored transactionally and the batch is acknowledged only after commit

#### Scenario: DNS event is replayed
- **WHEN** the same agent-scoped DNS event ID is delivered again
- **THEN** storage, grouping, release summaries, and first-seen work remain idempotent

#### Scenario: Correlated context is stored
- **WHEN** a valid connection occurrence contains qualified DNS context
- **THEN** the immutable evidence time, expiry, confidence, ambiguity, and bounded names are retained without later TTL processing rewriting history
