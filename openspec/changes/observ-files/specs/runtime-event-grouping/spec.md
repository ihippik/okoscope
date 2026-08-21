## ADDED Requirements

### Requirement: Deterministic file activity grouping
The server SHALL group file activity using trusted tenant/workload scope, fingerprint version, event kind, normalized process command, and operation paths; create, modify, and delete SHALL use their path, while rename SHALL use its ordered old and new paths plus replacement identity.

#### Scenario: Equivalent modifications recur
- **WHEN** the same selected workload and process repeatedly produce accepted `file.modify` events for the same normalized path
- **THEN** the occurrences update one deterministic group despite different inode, mount, Pod, container, PID, event ID, or release values

#### Scenario: File semantic identity differs
- **WHEN** event kind, process command, path, either rename path, replacement identity, or trusted grouping scope differs
- **THEN** the events are assigned to distinct groups

#### Scenario: Release attribution changes
- **WHEN** identical file activity occurs in two resolved Application releases
- **THEN** cross-release group identity remains stable and release-scoped summaries compare the behavior

### Requirement: Safe file activity summaries
Each file activity group SHALL expose only its operation, bounded normalized path fields, replacement identity when applicable, and normalized process command as its semantic summary, and first-seen work MUST NOT include file contents, written bytes, host paths, or excluded paths.

#### Scenario: First file behavior is grouped
- **WHEN** the first accepted file occurrence creates a runtime group
- **THEN** exactly one first-seen outbox record is created with the safe file semantic summary

