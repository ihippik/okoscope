## ADDED Requirements

### Requirement: Durable canonical file activity storage
The server SHALL validate and durably store typed file create, modify, delete, and rename evidence with complete bounded normalized paths, operation-specific field invariants, existing trusted tenant/workload/release attribution, distinct observation and receive times, and existing replay guarantees.

#### Scenario: Valid file event is committed
- **WHEN** a valid attributed file activity event passes ingestion validation
- **THEN** its canonical payload is committed transactionally and acknowledged only after commit

#### Scenario: Malformed file event is submitted
- **WHEN** a file payload contains an empty, relative, non-normalized, truncated, oversized, NUL-containing, or operation-inconsistent path
- **THEN** the server rejects the batch without partially committing it or logging path contents

#### Scenario: File event is replayed
- **WHEN** the same agent-scoped file activity event ID is delivered again
- **THEN** raw storage and all downstream materialization remain idempotent

