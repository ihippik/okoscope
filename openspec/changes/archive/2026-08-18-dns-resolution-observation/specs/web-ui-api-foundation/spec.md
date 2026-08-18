## ADDED Requirements

### Requirement: Web APIs expose safe typed DNS evidence
Authenticated tenant-scoped APIs and OpenAPI SHALL expose closed typed DNS query/response summaries and occurrences plus optional qualified connection DNS context with bounded names, answers, TTL/evidence timestamps, confidence, and ambiguity.

#### Scenario: Client retrieves DNS behavior
- **WHEN** an authenticated principal lists or opens owned DNS groups and occurrences
- **THEN** responses contain only documented bounded fields with existing pagination, request correlation, lifecycle, release, and tenant-safe not-found behavior

### Requirement: Web UI renders DNS evidence safely
The Web UI SHALL render canonical names and answers as inert text, explain confidence, ambiguity, cache/encryption limitations and unavailable states, and MUST NOT automatically navigate, resolve, or make untrusted names clickable.

#### Scenario: User investigates correlated connection context
- **WHEN** a connection occurrence contains one or more qualified DNS names
- **THEN** the UI presents them separately from the IP destination with accessible labels and no automatic request to those names

#### Scenario: Forbidden DNS content is supplied
- **WHEN** an API response or test fixture contains unsupported packet, EDNS, header, body, URL, or secret fields
- **THEN** generated types reject the shape where possible and the UI does not render the forbidden content
