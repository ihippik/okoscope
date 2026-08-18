## ADDED Requirements

### Requirement: Versioned DNS event transport
The agent protocol SHALL add backward-compatible typed DNS query and response payloads, versioned UDP/TCP DNS capabilities, optional qualified DNS context on connection occurrences, and additive bounded DNS loss counters.

#### Scenario: DNS-capable agent connects
- **WHEN** an agent enables supported DNS observation
- **THEN** its hello advertises the exact DNS transport capabilities and heartbeats expose additive counters without changing existing capabilities

#### Scenario: Older participant exchanges messages
- **WHEN** an older agent or server encounters additive DNS fields or variants it does not understand
- **THEN** existing non-DNS session behavior remains compatible and no unknown payload is interpreted as trusted DNS evidence

#### Scenario: Malformed DNS payload is submitted
- **WHEN** typed DNS fields violate canonical name, answer, TTL, response, attribution, or correlation invariants
- **THEN** the server rejects the batch before acknowledgement without logging raw names or packet bytes
