## ADDED Requirements

### Requirement: Agent platform metadata is retained on registration
The server SHALL normalize and durably retain the latest usable architecture and Linux kernel release supplied by an agent hello for the worker resolved by the authenticated session, and SHALL refresh those values when that worker reconnects.

#### Scenario: Agent supplies platform metadata
- **WHEN** an authenticated compatible agent establishes a session with non-empty architecture and kernel release values
- **THEN** the server stores the normalized values on the resolved agent worker before accepting runtime events

#### Scenario: Existing worker reconnects after a kernel change
- **WHEN** the same Cluster and node identity reconnects with a different usable kernel release
- **THEN** the existing agent worker retains its identity and its current kernel release is updated to the newly reported value

#### Scenario: Platform metadata is absent or unknown
- **WHEN** a compatible agent omits a platform value or sends an empty or recognized unknown sentinel
- **THEN** the server stores that value as unavailable without rejecting the otherwise valid session
