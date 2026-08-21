## ADDED Requirements

### Requirement: Versioned file activity transport
The agent protocol SHALL add backward-compatible typed file activity payloads and an exact `file.activity.syscall-path/v1` capability, and the server MUST validate operation, bounded normalized absolute syscall-path fields, rename fields, and replacement identity before acknowledging a batch.

#### Scenario: File-capable agent connects
- **WHEN** an agent enables supported file observation
- **THEN** its hello advertises `file.activity.syscall-path/v1` and valid typed file events use existing batch, durability, replay, and acknowledgement guarantees

#### Scenario: File observation is disabled or unavailable
- **WHEN** file observation is not configured or cannot be attached
- **THEN** the hello omits `file.activity.syscall-path/v1` while unrelated capabilities remain unchanged

#### Scenario: Older participant exchanges messages
- **WHEN** an older agent or server encounters additive file variants or counters it does not understand
- **THEN** existing non-file session behavior remains compatible and no unknown payload is interpreted as trusted file evidence

### Requirement: File observation loss reporting
Agent heartbeats SHALL report additive monotonic counters for file filtering, path resolution, path oversize, unsupported object, attribution, aggregation capacity, decode, rate limiting, and kernel output loss without using paths or workload values as labels.

#### Scenario: File observation loses evidence
- **WHEN** a bounded file observation stage drops or rejects activity
- **THEN** the next heartbeat exposes the corresponding counter while retaining protocol compatibility
