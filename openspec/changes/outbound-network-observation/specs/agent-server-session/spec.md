## ADDED Requirements

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
