## ADDED Requirements

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
