## ADDED Requirements

### Requirement: TCP listener transitions are typed events
The agent SHALL emit `network.listen` only when a selected workload successfully transitions an IPv4 or IPv6 TCP socket to listening state, and the event SHALL contain the effective canonical local address, non-zero local port, TCP transport, process identity, observation time, and trusted Kubernetes attribution.

#### Scenario: Workload opens a TCP listener
- **WHEN** a process in a selected workload successfully starts listening on an IPv4 or IPv6 TCP socket
- **THEN** the agent emits one attributed `network.listen` event containing the effective kernel-observed endpoint

#### Scenario: Listen operation fails
- **WHEN** a selected process attempts to listen but no socket reaches listening state
- **THEN** the agent emits no `network.listen` event and records any applicable bounded failure counter

#### Scenario: Kernel assigns the port
- **WHEN** a selected process binds port zero and successfully starts listening
- **THEN** the event contains the non-zero effective port assigned by the kernel rather than the requested zero value

### Requirement: Accepted TCP connections are typed events
The agent SHALL emit `network.accept` only when a selected workload successfully accepts an IPv4 or IPv6 TCP connection, and the event SHALL contain canonical local and remote endpoints, TCP transport, process identity, observation time, and trusted Kubernetes attribution.

#### Scenario: Workload accepts an inbound connection
- **WHEN** a TCP connection is successfully accepted by a process in a selected workload
- **THEN** the agent emits one `network.accept` occurrence with its local and remote socket endpoints

#### Scenario: Ingress traffic is not accepted
- **WHEN** a SYN, rejected attempt, or other ingress packet does not result in an accepted application connection
- **THEN** the agent does not describe it as `network.accept`

### Requirement: Inbound capture has a strict privacy boundary
Inbound observation MUST NOT capture packet payloads, application protocol contents, HTTP or TLS fields, credentials, socket buffers, or unrestricted process arguments, and remote endpoints MUST NOT appear in metrics labels or unbounded logs.

#### Scenario: Accepted traffic contains sensitive contents
- **WHEN** an accepted connection exchanges sensitive application data
- **THEN** the emitted evidence contains only the typed socket and common attribution fields and none of the exchanged contents

### Requirement: Inbound observation remains bounded and explicit
The agent SHALL apply fixed kernel map and ring limits, existing queue and global rate limits, a dedicated accepted-connection rate limit, and monotonic reason counters for unsupported family, decoding, attribution, kernel output, rate, and any required correlation loss.

#### Scenario: Accepted-connection rate is exceeded
- **WHEN** accepted connections exceed the configured dedicated or global event rate
- **THEN** the agent remains operational, drops excess evidence according to the documented policy, and increments the matching monotonic counter

#### Scenario: Trusted attribution is unavailable
- **WHEN** a candidate inbound event cannot be reliably attributed to a selected workload
- **THEN** the agent sends no event and increments the attribution-failure counter without logging endpoints as labels

### Requirement: Unsupported kernels do not claim complete observation
The agent SHALL advertise listener and accepted-connection capabilities independently only after their required hooks and trustworthy attribution have been verified on the running kernel.

#### Scenario: Listener hook is supported but accept attribution is not
- **WHEN** the running kernel supports trusted listener observation but not trusted accepted-connection attribution
- **THEN** the agent may advertise `network.listen/v1` but MUST omit `network.accept/v1` and remain unready if accept observation is configured as required
