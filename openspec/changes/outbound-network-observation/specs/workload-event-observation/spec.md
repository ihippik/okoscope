## ADDED Requirements

### Requirement: Opt-in outbound network configuration
The agent SHALL accept a strict `observation.network.connect` boolean that defaults to `false`, SHALL attach network probes only when it is true, and SHALL continue applying configured workload selectors and global safety limits to network events.

#### Scenario: Network observation is omitted
- **WHEN** an otherwise valid configuration omits `observation.network` or sets `connect` to `false`
- **THEN** the agent does not attach connect probes and reports no network capability

#### Scenario: Network observation is enabled
- **WHEN** a valid configuration sets `observation.network.connect` to `true`
- **THEN** the agent validates kernel support, attaches the required entry and exit probes, and reports the versioned network capability

#### Scenario: Configuration contains unknown network fields
- **WHEN** network observation configuration contains an unknown field or unsupported mode
- **THEN** startup fails with a safe validation error before any probe is attached

### Requirement: Bounded connect correlation
The agent SHALL use bounded kernel state to correlate connect entry and exit, SHALL delete correlation state after completion, and SHALL expose capacity, decode, unsupported-family, and kernel-output losses through bounded counters.

#### Scenario: Correlation capacity is exhausted
- **WHEN** a connect entry cannot be retained because the bounded correlation map is full
- **THEN** the agent increments the correlation-capacity counter and emits no partial or mismatched event

#### Scenario: Connect exit has no retained entry
- **WHEN** a connect exit is observed without matching retained entry state
- **THEN** the agent increments a bounded correlation-miss counter and emits no event
