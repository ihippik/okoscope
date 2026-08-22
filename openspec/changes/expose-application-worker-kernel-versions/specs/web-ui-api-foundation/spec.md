## ADDED Requirements

### Requirement: OpenAPI describes Application worker platform discovery
The published OpenAPI 3.1 contract SHALL describe the authenticated bounded Application worker collection endpoint with concrete request, pagination, success, validation, authorization, and tenant-safe not-found schemas.

#### Scenario: UI generates worker discovery client types
- **WHEN** the separate Web UI generates a client from the published contract
- **THEN** it receives typed worker identity, Cluster context, agent version, nullable architecture and kernel release, observation timestamps, agent last-seen timestamp, and opaque pagination cursor fields

#### Scenario: Contract route coverage is validated
- **WHEN** repository API contract checks run
- **THEN** the implemented Application worker route and operation identifier are covered and its collection response contains no unbounded generic object
