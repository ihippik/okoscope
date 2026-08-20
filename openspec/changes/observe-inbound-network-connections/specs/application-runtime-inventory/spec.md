## ADDED Requirements

### Requirement: Application inventory exposes inbound TCP endpoints
The Application runtime inventory SHALL expose an inbound endpoint item identified by identity version, TCP transport, address family, canonical local address, and non-zero local port across contributing clusters, workloads, Pods, containers, and releases.

#### Scenario: Equivalent listener appears in multiple deployments
- **WHEN** the same canonical TCP endpoint is observed for one Application across different trusted deployment scopes
- **THEN** one application-scoped inbound endpoint item aggregates the sightings and preserves navigation to contributing groups and occurrences

#### Scenario: Local endpoint changes
- **WHEN** the address family, canonical local address, or local port differs
- **THEN** the evidence projects to a distinct inbound endpoint inventory item

### Requirement: Listener and accept evidence remain distinguishable
An inbound endpoint item SHALL report bounded evidence indicating whether listener events, accepted-connection events, or both contributed within the selected scope, and MUST NOT claim listener observation solely because a declared Kubernetes port exists.

#### Scenario: Observation begins after listener startup
- **WHEN** accepted connections are observed for an endpoint but no listener transition was retained in the selected evidence window
- **THEN** the inventory may show the endpoint with `accept_observed` evidence while `listener_observed` remains false or unknown

#### Scenario: Kubernetes manifest declares a port
- **WHEN** a container or Service declares a port but no runtime listener or accepted connection is observed
- **THEN** the declaration alone does not create an observed inbound runtime inventory item

### Requirement: Remote clients do not create inventory identity
Remote addresses and remote ports from `network.accept` occurrences MUST NOT enter inbound endpoint inventory identity, facet values, default summaries, or first-seen notification identity, and SHALL remain available only as bounded authorized raw evidence.

#### Scenario: Many remote clients reach one endpoint
- **WHEN** one Application endpoint accepts connections from many distinct remote endpoints
- **THEN** the inventory retains one inbound endpoint item and does not create one item per client

### Requirement: Inbound inventory preserves scoped release evidence
Inbound endpoint items SHALL participate in existing release presence, observation-window, deployment-sighting, occurrence, pagination, and reconciliation semantics without treating accepted traffic volume as listener behavioral identity.

#### Scenario: Endpoint is compared across releases
- **WHEN** an authorized user selects a release and operational scope for an inbound endpoint
- **THEN** its presence state is derived from trusted listener evidence under existing observed, not-observed, and unknown rules, with accepted evidence presented separately
