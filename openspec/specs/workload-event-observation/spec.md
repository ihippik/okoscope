# workload-event-observation Specification

## Purpose
Defines configurable eBPF observation, Kubernetes attribution, and workload filtering performed by the agent.

## Requirements

### Requirement: Agent configuration uses versioned YAML
The agent SHALL load a YAML configuration containing `apiVersion`, `kind`, server connection information, workload selection, enabled event types, and local safety limits.

#### Scenario: Valid configuration starts observation
- **WHEN** the agent receives a supported YAML document with at least one selected workload and one supported event type
- **THEN** it validates the document and starts only the requested observation capabilities

#### Scenario: Unknown or invalid configuration is rejected
- **WHEN** the YAML contains an unsupported version, unknown field, unknown syscall name, or invalid selector
- **THEN** the agent exits or remains unready with an actionable validation error and starts no partial observation

### Requirement: Workload observation is explicitly scoped
The agent SHALL support selectors for Kubernetes namespace, top-level workload kind, workload name, and optional labels, and SHALL send only events attributed to a matching workload.

#### Scenario: Event belongs to selected deployment
- **WHEN** an observed process belongs to a Pod owned by a Deployment matching the configured selector
- **THEN** the agent emits an attributed runtime event for that Deployment

#### Scenario: Event belongs to another deployment
- **WHEN** an observed process belongs to a workload that does not match any configured selector
- **THEN** the agent does not send that event and increments an observable filtered counter

### Requirement: Process execution is a typed event
The agent SHALL observe successful process execution as `process.exec` with observation time, cgroup identifier, PID/TGID, executable identity available from the selected hook, and parent process identity when reliably available.

#### Scenario: Shell executes in selected workload
- **WHEN** `/bin/sh` is successfully executed inside a selected workload
- **THEN** the agent emits one typed `process.exec` event attributed to that workload

### Requirement: Named syscall allowlist
The agent SHALL observe only explicitly configured syscall names, resolve them for the running architecture, and SHALL NOT accept wildcard or numeric syscall configuration in the MVP.

#### Scenario: Configured syscall occurs
- **WHEN** a selected workload invokes a supported syscall included by name in configuration
- **THEN** the agent emits a typed syscall event containing the canonical syscall name

#### Scenario: Unconfigured syscall occurs
- **WHEN** a selected workload invokes a syscall not present in configuration
- **THEN** the agent does not submit an event for that syscall

### Requirement: Kubernetes attribution is complete before delivery
Each delivered runtime event SHALL include cluster and node identity plus namespace, Pod UID/name, container identity, top-level workload UID/kind/name, Project, and Application mapping.

#### Scenario: Attribution cache resolves a workload
- **WHEN** cgroup and container identity resolve through the Kubernetes metadata cache and owner chain
- **THEN** the emitted event contains the resolved immutable UIDs and human-readable names

#### Scenario: Attribution is uncertain
- **WHEN** the agent cannot reliably associate an event with a configured workload
- **THEN** it does not deliver the event and increments an unattributed-event counter

### Requirement: Sensitive contents are excluded
The MVP agent MUST NOT capture process environment variables, file contents, network payloads, or unrestricted process arguments.

#### Scenario: Process execution is observed
- **WHEN** a process executes with secrets in its environment or arguments
- **THEN** the emitted event contains none of its environment and no unrestricted argument list

### Requirement: Unsupported platforms are visible
The agent SHALL report its kernel, architecture, version, and observation capabilities and SHALL remain unready when a configured required capability cannot be provided.

#### Scenario: Required probe is unsupported
- **WHEN** the configured kernel cannot support a required observation capability
- **THEN** the agent reports the unsupported capability and does not claim complete observation

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
