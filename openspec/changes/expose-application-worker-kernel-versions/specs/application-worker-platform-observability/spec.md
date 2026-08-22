## ADDED Requirements

### Requirement: Application workers expose current platform metadata
The server SHALL expose each distinct agent worker that has accepted runtime-event evidence for an authenticated principal's owned Application, including worker and Cluster identity, agent version, nullable architecture and Linux kernel release, Application observation bounds, and agent last-seen time.

#### Scenario: Application spans heterogeneous workers
- **WHEN** two workers reporting different kernel releases have accepted runtime events for the same owned Application
- **THEN** the Application worker collection returns separate items preserving each worker's reported kernel release

#### Scenario: Worker metadata is unavailable
- **WHEN** an existing row or compatible older agent has no usable architecture or kernel release
- **THEN** the corresponding response fields are null and the worker remains present in the collection

#### Scenario: Connected worker has no Application evidence
- **WHEN** an agent is known in the Application's Cluster but has no accepted runtime event for that Application
- **THEN** the agent is not included in the Application worker collection

### Requirement: Application worker discovery is tenant safe and bounded
The Application worker collection SHALL derive Organization scope from authentication, require the Application to belong to the Project in the request path, and use bounded deterministic cursor pagination ordered by Application observation recency and stable worker identity.

#### Scenario: Principal lists owned Application workers
- **WHEN** an authenticated principal requests workers for an Application owned by the Project and Organization in scope
- **THEN** the server returns at most the bounded page limit in descending last-observed order with an opaque next cursor when more workers exist

#### Scenario: Application is outside the path scope
- **WHEN** a principal requests a foreign Application or an Application under a different Project
- **THEN** the server returns the tenant-safe not-found response without exposing worker or ownership data

#### Scenario: Cursor or limit is invalid
- **WHEN** a caller supplies a malformed cursor, a non-positive limit, or a limit above the supported maximum
- **THEN** the server returns the standard validation error without running an unbounded query

### Requirement: Worker platform values are safe display data
The API SHALL expose bounded platform strings as inert descriptive data and MUST NOT interpret a kernel release as a distribution, support guarantee, vulnerability result, or compatibility verdict.

#### Scenario: Client renders reported platform metadata
- **WHEN** the Web UI displays a worker's node name, architecture, or kernel release
- **THEN** it renders the value as inert text and represents null as unavailable
