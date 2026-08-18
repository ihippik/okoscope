## ADDED Requirements

### Requirement: Typed outbound connection attempts
The system SHALL represent each supported outbound `connect()` attempt as a typed `network.connect` event containing canonical destination address family, destination IP address, destination port, process identity, trusted Kubernetes attribution, observation time, and an outcome of `succeeded`, `in_progress`, or `failed` with a bounded errno value when applicable.

#### Scenario: Blocking connection succeeds
- **WHEN** a process in a selected workload completes an IPv4 or IPv6 `connect()` syscall with a zero result
- **THEN** the agent emits one `network.connect` event for the destination with outcome `succeeded`

#### Scenario: Non-blocking connection continues
- **WHEN** a supported `connect()` syscall returns `EINPROGRESS`
- **THEN** the agent emits one attempt with outcome `in_progress` and does not claim that a connection was established

#### Scenario: Connection attempt fails
- **WHEN** a supported `connect()` syscall returns another negative errno
- **THEN** the agent emits one attempt with outcome `failed` and the canonical bounded errno without an error string sourced from the observed process

### Requirement: Privacy-bounded network metadata
Network observation MUST NOT capture packet payloads, socket-buffer contents, HTTP data, TLS contents, DNS query names, Unix-domain paths, source ephemeral ports, process environment variables, or unrestricted process arguments.

#### Scenario: Workload transmits application data
- **WHEN** an observed process sends sensitive application data before or after a connection attempt
- **THEN** the network event contains only the allowed connection-attempt metadata and none of the transmitted content

#### Scenario: Unsupported socket family is used
- **WHEN** a selected process calls `connect()` with a family other than `AF_INET` or `AF_INET6`
- **THEN** the agent emits no runtime event and increments a bounded unsupported counter without recording the address bytes or path

### Requirement: Safe network investigation
Authenticated Project users SHALL be able to list, filter, inspect, and compare grouped `network.connect` behavior through existing runtime-group and release-diff APIs and Web UI views using safe semantic destination and outcome fields.

#### Scenario: User investigates a network group
- **WHEN** an authenticated principal opens an owned `network.connect` runtime group
- **THEN** the API and Web UI show the canonical destination, process command, occurrence history, release attribution, and per-occurrence outcome without packet contents or unbounded kernel data

#### Scenario: User filters network behavior
- **WHEN** an authenticated principal filters an owned Application's runtime groups by event kind `network.connect`
- **THEN** only matching tenant-scoped groups are returned using existing bounded cursor pagination

### Requirement: Deployed network observation acceptance
The release SHALL provide an automated and operator-readable acceptance flow that proves selected successful and failed outbound attempts, IPv4 and IPv6 decoding where supported, deduplication, grouping, API investigation, and exclusion of unselected workloads and unsupported families.

#### Scenario: Operator runs the controlled acceptance flow
- **WHEN** an operator enables network observation for the documented fixture and runs controlled connection attempts
- **THEN** the flow traces expected events from eBPF capture through durable storage and UI-facing APIs and reports bounded drop counters without exposing payload data
