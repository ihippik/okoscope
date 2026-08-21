## ADDED Requirements

### Requirement: File activity is explicit and opt-in
The agent SHALL accept a strict default-disabled `observation.files` configuration with a non-empty operation set drawn from `create`, `modify`, `delete`, and `rename`, a required non-empty `includePaths` list, and an optional `excludePaths` list.

#### Scenario: File observation is omitted
- **WHEN** an otherwise valid configuration omits `observation.files` or sets `enabled` to false
- **THEN** the agent attaches no file probes and advertises no file activity capability

#### Scenario: File observation configuration is unsafe
- **WHEN** file observation is enabled with no included path, a relative or non-normalized path, an unknown operation, an unknown field, an empty path, or a path containing NUL
- **THEN** startup fails before any file probe is attached

### Requirement: Only successful regular-file activity is typed
The agent SHALL emit successful regular-file activity as `file.create`, `file.modify`, `file.delete`, or `file.rename` and SHALL exclude directories, symbolic links as objects, sockets, devices, and failed attempts.

#### Scenario: New regular file is created
- **WHEN** a selected workload successfully creates a previously absent regular file in configured path scope
- **THEN** the agent emits one attributed `file.create` event for the resolved file path

#### Scenario: Existing file is truncated
- **WHEN** a selected workload successfully truncates an existing in-scope regular file
- **THEN** the activity contributes to `file.modify` and does not emit `file.create`

#### Scenario: Non-regular object changes
- **WHEN** a selected workload changes a directory, symbolic link object, socket, or device
- **THEN** the agent emits no file activity event for that object

### Requirement: Paths use the experimental syscall-path profile
Every delivered file activity event SHALL contain a bounded normalized absolute pathname argument supplied by the acting process or an exact pathname retained from its successful tracked open, SHALL identify `syscall_argument_v1` semantics in the versioned contract, and MUST NOT claim symlink resolution or filesystem-object canonicalization.

#### Scenario: Syscall supplies a bounded absolute path
- **WHEN** a selected operation supplies a complete normalized absolute pathname argument within the configured maximum
- **THEN** the event contains that pathname with `syscall_argument_v1` semantics

#### Scenario: Path is unsupported
- **WHEN** a pathname is relative, non-normalized, unreadable, unterminated, or exceeds the maximum
- **THEN** the agent emits no event and increments the matching monotonic reason counter

### Requirement: Include and exclude filters use path boundaries
The agent SHALL include activity only when its normalized path equals or descends from an `includePaths` entry, SHALL evaluate prefixes on path-component boundaries, and SHALL give `excludePaths` precedence without disclosing excluded paths.

#### Scenario: Included descendant changes
- **WHEN** `/app/data/report.csv` changes and `/app/data` is included without a matching exclusion
- **THEN** the activity is eligible for emission

#### Scenario: Textual prefix is not a path ancestor
- **WHEN** `/app/database/report.csv` changes and only `/app/data` is included
- **THEN** the activity is filtered because the prefix does not end at a component boundary

#### Scenario: Exclusion overrides inclusion
- **WHEN** `/app/data/cache/item` changes while `/app/data` is included and `/app/data/cache` is excluded
- **THEN** no event exposes the excluded path and the filtered counter increases

### Requirement: File modifications are aggregated for five seconds
The agent SHALL aggregate successful modifications by trusted workload UID, container identity, process TGID, and tracked file descriptor generation for a fixed code-defined five-second window and SHALL emit at most one `file.modify` occurrence per completed window using the pathname retained from the successful tracked open.

#### Scenario: File is written repeatedly
- **WHEN** one selected workload modifies the same mounted inode repeatedly during one five-second window
- **THEN** one `file.modify` event represents that window without file contents, byte counts, or individual write calls

#### Scenario: Structural activity follows modification
- **WHEN** a pending modified inode is renamed or deleted
- **THEN** the agent flushes its pending `file.modify` before emitting the structural event

#### Scenario: Aggregation state is exhausted
- **WHEN** bounded aggregation state cannot retain another inode
- **THEN** no misleading partial aggregation is emitted and a capacity-loss counter increases

### Requirement: Rename preserves safe scope semantics
An in-scope rename SHALL be a distinct `file.rename` event containing complete old and new paths; movement into scope SHALL be represented as `file.create`, movement out of scope SHALL be represented as `file.delete`, and excluded paths MUST NOT be exposed.

#### Scenario: Both rename paths are in scope
- **WHEN** a selected workload successfully renames a regular file and both paths pass the filters
- **THEN** one `file.rename` event contains the old path, new path, and replacement state when syscall flags prove it; otherwise replacement state is unknown

#### Scenario: File enters observed scope
- **WHEN** a regular file is renamed from outside observed scope to an included non-excluded path
- **THEN** the agent emits `file.create` containing only the new path

#### Scenario: File leaves observed scope
- **WHEN** a regular file is renamed from an included non-excluded path to outside observed scope
- **THEN** the agent emits `file.delete` containing only the old path

#### Scenario: Neither path is observable
- **WHEN** neither rename path passes the filters
- **THEN** the agent emits no event and reveals neither path

### Requirement: File observation resources and losses are bounded
The agent SHALL bound kernel records, path size, path work, aggregation entries, ring output, userspace queues, batches, and event rate and SHALL expose monotonic reason counters without paths, workload names, or process values as metric labels.

#### Scenario: File event cannot be retained
- **WHEN** any bounded file observation stage rejects or drops an event
- **THEN** the corresponding path, capacity, attribution, filtering, decode, rate-limit, or kernel-output counter increases
