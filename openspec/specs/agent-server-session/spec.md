# agent-server-session Specification

## Purpose
Defines authenticated, versioned, bounded, and bidirectional communication between Okoscope agents and the server.

## Requirements

### Requirement: Agent initiates a bidirectional session
The agent SHALL establish one long-lived versioned bidirectional gRPC session to the server and send a hello message before event batches.

#### Scenario: Compatible agent connects
- **WHEN** an authenticated agent sends a hello with compatible protocol version, agent version, node identity, and capabilities
- **THEN** the server accepts the session and returns its resolved Organization, Cluster, agent identity, and supported protocol version

#### Scenario: Incompatible protocol connects
- **WHEN** an agent sends an unsupported protocol version
- **THEN** the server rejects the session with a machine-readable compatibility error

### Requirement: Session authentication determines scope
The server SHALL authenticate an agent using a server-provisioned cluster credential and SHALL bind the resulting session to exactly one Organization and Cluster.

#### Scenario: Valid cluster credential is presented
- **WHEN** an agent connects using an active credential issued for a Cluster
- **THEN** the server binds the session to that credential's Organization and Cluster

#### Scenario: Invalid credential is presented
- **WHEN** an agent presents a missing, unknown, revoked, or incorrectly scoped credential
- **THEN** the server rejects the session before accepting runtime events

### Requirement: Event delivery is batched and acknowledged
The agent SHALL send bounded event batches with an agent-scoped sequence, and the server SHALL acknowledge a batch only after all accepted events in its transaction are durably committed.

#### Scenario: Batch transaction commits
- **WHEN** a valid batch is persisted successfully
- **THEN** the server returns an acknowledgement containing the batch sequence

#### Scenario: Batch transaction fails
- **WHEN** persistence fails
- **THEN** the server does not acknowledge the batch as committed and returns or records an actionable failure

### Requirement: Duplicate events are idempotent
Every runtime event SHALL carry an agent-generated event ID, and replaying the same event ID in the same agent scope MUST NOT create an additional stored runtime event.

#### Scenario: Agent resends a previously committed event
- **WHEN** the server receives the same agent identity and event ID again after reconnect
- **THEN** it treats the event as already accepted and stores no duplicate row

### Requirement: Agent resources are bounded
The agent SHALL use configured bounded buffering and SHALL expose counts of events dropped due to filtering, attribution failure, unsupported capabilities, or capacity exhaustion.

#### Scenario: Server is unavailable and memory buffer fills
- **WHEN** additional events arrive after the bounded queue reaches capacity
- **THEN** the agent remains operational, drops events according to a documented policy, and reports the loss count after reconnection

### Requirement: Server-to-agent messages are typed
The protocol SHALL use explicit typed server-to-agent message variants and MUST NOT expose a generic command, shell, or arbitrary-code execution mechanism.

#### Scenario: Unknown control message is received
- **WHEN** an agent receives a server message type or capability it does not support
- **THEN** it rejects or ignores that message safely and reports an unsupported result without executing arbitrary content

### Requirement: Production transport is encrypted
The agent and server SHALL require TLS outside an explicitly enabled development mode.

#### Scenario: Plaintext production connection is attempted
- **WHEN** an agent attempts a plaintext connection while development mode is disabled
- **THEN** the connection is rejected

### Requirement: Versioned network event transport
The agent protocol SHALL add a backward-compatible typed `network.connect` payload and a `network.connect/v1` capability, and the server MUST reject malformed addresses, ports, outcomes, or outcome/errno combinations before acknowledging a batch as committed.

#### Scenario: Compatible network event is delivered
- **WHEN** an accepted agent advertising `network.connect/v1` sends a valid typed network event
- **THEN** the server validates and processes it using the existing batch durability, acknowledgement, replay, and event-ID deduplication guarantees

#### Scenario: Agent does not advertise network support
- **WHEN** network observation is disabled or unavailable
- **THEN** the hello message omits `network.connect/v1` while existing process and syscall capabilities continue unchanged

#### Scenario: Network payload is malformed
- **WHEN** a network payload has an invalid address length, zero destination port, unsupported family, unknown outcome, or inconsistent errno
- **THEN** the server rejects the invalid batch without partially acknowledging it or logging raw payload bytes

### Requirement: Network observation loss reporting
Agent heartbeats SHALL report additive bounded counters for connect correlation capacity, correlation misses, address decode failures, unsupported families, and kernel output loss without using destination or workload values as metric labels.

#### Scenario: Network observation loses an event
- **WHEN** any bounded network capture stage drops or rejects an event
- **THEN** the next heartbeat exposes the corresponding monotonic counter and existing agents remain protocol-compatible
