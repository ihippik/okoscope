## ADDED Requirements

### Requirement: Inbound network observation is independently opt-in
The agent SHALL accept strict `observation.network.listen` and `observation.network.accept` booleans that default to `false`, SHALL attach only the hooks required by enabled modes, and SHALL continue applying configured workload selectors and global safety limits.

#### Scenario: Inbound options are omitted
- **WHEN** an otherwise valid configuration omits both inbound network options
- **THEN** the agent attaches no inbound hooks and advertises neither inbound capability

#### Scenario: Only listener observation is enabled
- **WHEN** `observation.network.listen` is true and `observation.network.accept` is false
- **THEN** the agent observes listener transitions without collecting accepted-connection events

#### Scenario: Unknown inbound configuration is supplied
- **WHEN** the network observation configuration contains an unknown inbound field or invalid limit
- **THEN** startup fails safely before any partial observation begins
