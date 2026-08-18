## ADDED Requirements

### Requirement: Opt-in bounded DNS capture configuration
The agent SHALL accept strict default-disabled DNS observation configuration and SHALL attach cgroup-aware packet probes only when enabled, while applying existing workload selectors and global rate, queue, batch, and replay limits.

#### Scenario: DNS observation is disabled
- **WHEN** DNS configuration is omitted or explicitly disabled
- **THEN** no DNS packet probe is attached and existing process, syscall, and connection observation remain unchanged

#### Scenario: Required packet capability is unavailable
- **WHEN** DNS observation is enabled but the kernel cannot provide required packet access or trusted cgroup attribution
- **THEN** the agent remains unready with a safe capability error rather than emitting unattributed DNS evidence

### Requirement: DNS parser and correlation resources are bounded
The agent SHALL enforce fixed limits for captured bytes, parser work, pending transactions, TCP reassembly, answers, names, TTL, and event rate and SHALL expose monotonic reason counters for every discarded or incomplete DNS observation.

#### Scenario: Malformed or oversized DNS input is observed
- **WHEN** a message has invalid compression, bounds, framing, labels, counts, or exceeds a configured hard limit
- **THEN** no partial event is emitted and the matching bounded counter increments without logging packet or name contents
