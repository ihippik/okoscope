## ADDED Requirements

### Requirement: Inventory distributions cover the complete filtered scope
The server SHALL expose `GET /api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/distribution` for an owned Application. The endpoint MUST require one closed `kind` value from `process`, `destination`, `domain`, or `syscall`; accept the same release, Kubernetes, observation-window, and bounded search filters with the same semantics as Runtime Inventory list and summary; and compute results from the complete matching identity set rather than a cursor page.

#### Scenario: Distribution is filtered
- **WHEN** an authenticated caller requests a distribution with any valid combination of release, cluster, namespace, workload kind, workload name, container name, observation-window, and search filters
- **THEN** every total and entry is computed only from identities and occurrences matching all supplied filters under the owned Project and Application

#### Scenario: Distribution is empty
- **WHEN** no inventory identity matches the effective filters
- **THEN** the server returns `identity_version` 1, the requested kind, zero item and occurrence totals, an empty `entries` array, and `other: null`

#### Scenario: Distribution request is invalid or unauthorized
- **WHEN** kind, limit, filter bounds, authentication, Project ownership, or Application ownership is invalid
- **THEN** the server returns the standard correlated 400, 401, or tenant-safe 404 error with `request_id` and no cross-tenant data

### Requirement: Inventory distribution top-N accounting is exact and deterministic
The distribution endpoint SHALL accept `limit` with default 5, minimum 1, and maximum 10. It SHALL order at most `limit` entries by `occurrence_count` descending and then stable `identity_token` ascending, and the entries plus optional `other` bucket MUST exactly cover `total_item_count` and `total_occurrence_count` without loading all matching records into application memory.

#### Scenario: Matching identities exceed the limit
- **WHEN** more identities match than the effective limit
- **THEN** the server returns the first deterministic top-N entries and an `other` bucket whose item and occurrence counts equal the exact remainder

#### Scenario: Matching identities fit within the limit
- **WHEN** the number of matching identities does not exceed the effective limit
- **THEN** the server returns every identity as an entry and returns `other: null`

#### Scenario: Occurrence counts are tied
- **WHEN** two entries have equal occurrence counts
- **THEN** their order is stable by ascending opaque identity token across repeated requests over unchanged data

### Requirement: Distribution entries expose safe typed identity summaries
Each distribution entry SHALL contain identity version 1 compatible identity information through an opaque `identity_token`, a kind-appropriate bounded `semantic_summary`, and exact item and occurrence counts. Observed semantic strings MUST remain inert text and MUST NOT be converted to HTML, Markdown, URLs, or navigation targets.

#### Scenario: Each supported kind is requested
- **WHEN** a caller requests a process, destination, domain, or syscall distribution
- **THEN** every entry contains only the documented semantic summary fields for that requested kind and a token selecting exactly that typed identity

#### Scenario: Observed text resembles markup or a link
- **WHEN** a semantic identity contains hostile, markup-like, URL-like, or traversal-like text
- **THEN** the API returns the bounded observed value as inert JSON text without generating markup or a link

### Requirement: Inventory list supports protected typed identity filtering
The Runtime Inventory list SHALL accept an optional `identity_token` string of length 1 through 1000 issued by the distribution endpoint. The server MUST authenticate or otherwise validate token integrity, expiry, identity version, typed identity, and compatibility with the trusted Organization, Project, Application, and requested kind derived independently from the request, and SHALL combine the selected identity with every other active list filter.

#### Scenario: Valid identity token is combined with filters
- **WHEN** a caller supplies a valid token for the owned scope together with other active inventory filters
- **THEN** the list returns only records matching both the token-selected typed identity and all other filters while retaining existing cursor pagination semantics

#### Scenario: Token contents attempt to change trusted scope
- **WHEN** a token claims another tenant, Project, Application, kind, or identity version
- **THEN** the server derives expected scope independently, returns a correlated 400 error with code `identity_token_scope_mismatch`, and reveals no data from the claimed scope

#### Scenario: Token is malformed or modified
- **WHEN** token syntax, signature, authenticated payload, or typed identity is invalid
- **THEN** the server returns a correlated 400 error with code `invalid_identity_token`

#### Scenario: Token is expired
- **WHEN** a structurally valid token is outside its documented validity period
- **THEN** the server returns a correlated 400 error with code `expired_identity_token`

#### Scenario: Identity filter changes during pagination
- **WHEN** the client changes `identity_token`
- **THEN** the client starts without the prior cursor and the server continues to validate any supplied cursor independently against the effective list scope

### Requirement: Inventory aggregate cost and contract are bounded and verifiable
The implementation SHALL use database aggregation, appropriate indexes, and bounded top-N retrieval rather than materializing all matching identities in application memory. OpenAPI and automated tests SHALL document all parameters, concrete schemas, examples, standard errors, token error codes, filter equivalence, tenant isolation, stable ordering, exact totals, and limit behavior for 0, 1, 5, 10, and 11.

#### Scenario: High-cardinality distribution is measured
- **WHEN** the performance workload exercises many identities, a wide observation range, and release and Kubernetes filters
- **THEN** the recorded evidence includes dataset volume, p50/p95/p99 latency, maximum response size, query plan, used indexes, accepted top-N maximum of 10, and discovered limitations

