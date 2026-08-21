## ADDED Requirements

### Requirement: File activity obeys workload scope and privacy boundaries
File activity observation SHALL apply existing Kubernetes workload selection and complete attribution before delivery and MUST NOT capture file contents, written bytes, host-translated paths, or paths rejected by configured filters.

#### Scenario: Selected workload changes included file
- **WHEN** a process attributed to a selected workload successfully changes an included non-excluded regular file
- **THEN** the resulting typed event contains complete Kubernetes attribution and only the permitted file metadata

#### Scenario: Unselected workload changes included path
- **WHEN** a process outside configured workload selection changes an otherwise included path
- **THEN** no event is delivered and an unattributed or filtered reason counter increases as applicable

### Requirement: Required file capability fails closed
The agent SHALL validate the syscall entry/exit tracepoints, bounded user-memory reads, correlation maps, and syscall-path semantics required for configured file observation and SHALL remain unready rather than advertise unavailable configured operation coverage.

#### Scenario: Syscall-path capability is unavailable
- **WHEN** file observation is enabled but the running kernel cannot provide every tracepoint required by its configured operations
- **THEN** the agent reports the unsupported capability, attaches no partial file observation set, and remains unready
