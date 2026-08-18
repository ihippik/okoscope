# runtime-event-storage Specification

## Purpose
Defines validation and durable PostgreSQL storage of accepted runtime events.

## Requirements

### Requirement: PostgreSQL stores accepted runtime events
The server SHALL persist each accepted runtime event in PostgreSQL with tenant and workload attribution, event identity, event kind, kernel and ingestion timestamps, normalized payload, and an optional server-resolved Application release reference.

#### Scenario: Event is accepted
- **WHEN** a supported runtime event passes ingestion validation
- **THEN** one row exists with its Organization, Project, Application, Cluster, Kubernetes attribution, agent identity, event ID, normalized event data, timestamps, and resolved release when available

#### Scenario: Event has no resolved release
- **WHEN** a supported runtime event has no release version or its version cannot be resolved inside the trusted Application scope
- **THEN** the event is persisted with a null release reference without losing its other attribution

### Requirement: Observation and ingestion times are distinct
The server SHALL preserve the agent-provided observation time and separately record a server-generated receive time.

#### Scenario: Event arrives after transport delay
- **WHEN** an event is received after its observation time
- **THEN** both timestamps are stored without replacing the observation time with the receive time

### Requirement: Ingestion validates event ownership and schema
The server SHALL validate protocol compatibility, event kind and normalized payload shape, derive Organization and Cluster from the authenticated session, verify Project and Application ownership, and resolve optional release versions inside that trusted Application before acknowledging the event as committed.

#### Scenario: Tenant identifiers disagree
- **WHEN** an event attempts to select an Organization or Cluster outside its authenticated session
- **THEN** the server does not persist it for the supplied tenant

#### Scenario: Project or Application is outside the session organization
- **WHEN** the event references a Project or Application that is not owned by the session Organization
- **THEN** the server rejects the event and does not persist it

#### Scenario: Release version belongs outside the trusted Application
- **WHEN** an event's release version cannot be resolved within the server-derived Organization, Project, and Application
- **THEN** the server persists the event without release attribution and records an observable attribution miss

### Requirement: Storage is idempotent
The database SHALL enforce uniqueness of event IDs within agent identity so retries cannot create duplicate rows.

#### Scenario: Concurrent duplicate delivery
- **WHEN** two transactions attempt to store the same agent identity and event ID
- **THEN** at most one runtime-event row exists after both transactions finish

### Requirement: Database schema is migration-managed
All PostgreSQL schema creation and changes SHALL be represented by ordered migrations that can be applied to an empty database and verified before the server becomes ready.

#### Scenario: Server starts with an empty database
- **WHEN** the documented migration procedure runs successfully
- **THEN** all tenant, agent, and runtime-event tables and constraints required by the MVP exist

#### Scenario: Required migrations are missing
- **WHEN** the server detects an incompatible database schema
- **THEN** it remains unready and reports the required migration state

### Requirement: Stored events are directly inspectable
The MVP SHALL document a PostgreSQL query that returns recent events for one Organization, Project, Application, event kind, and time range with their Kubernetes context.

#### Scenario: Operator verifies shell execution
- **WHEN** `/bin/sh` executes in the configured selected Deployment and ingestion succeeds
- **THEN** the documented query returns its `process.exec` event with the expected workload, Pod, and container identity

### Requirement: First-seen behavior is verifiable end to end
The MVP SHALL provide an automated integration test and an operator runbook that trace one selected workload event from ingestion through durable raw storage, deterministic grouping, first-seen outbox work, and UI-facing group and occurrence APIs.

#### Scenario: Selected workload produces new behavior
- **WHEN** a supported event with a previously unseen fingerprint is ingested for the selected Application
- **THEN** PostgreSQL contains one raw event, one group membership, one new group with matching discovery metadata, and one live first-seen outbox record

#### Scenario: Selected workload repeats behavior
- **WHEN** another distinct event with the same fingerprint is ingested
- **THEN** PostgreSQL contains the additional raw event and membership, the group count increases exactly once, and no second first-seen outbox record is created

#### Scenario: Operator follows the runbook
- **WHEN** an operator runs the documented verification against a deployed environment
- **THEN** the database and versioned API checks identify the same Application, group, representative event, and recent occurrence

### Requirement: Durable network event validation and storage
The server SHALL validate and persist accepted `network.connect` payloads using canonical textual IP addresses, unsigned destination ports, stable outcomes, and bounded errno values while preserving the existing trusted Organization, Project, Cluster, Application, agent, process, release, receive-time, and observation-time fields.

#### Scenario: Valid network event is committed
- **WHEN** a valid attributed network event is accepted in a batch
- **THEN** its canonical typed payload is durably stored in the same transaction and acknowledged only after commit

#### Scenario: Replayed network event is received
- **WHEN** an agent replays the same network event ID in its authenticated agent scope
- **THEN** storage and downstream grouping remain idempotent and do not create another occurrence

#### Scenario: Network event violates tenant scope
- **WHEN** a network event references a Project or Application outside the authenticated agent session organization
- **THEN** the server rejects it using the same trusted attribution rules as existing event kinds

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
