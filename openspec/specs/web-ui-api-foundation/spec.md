# web-ui-api-foundation Specification

## Purpose
Defines the browser-safe, versioned HTTP API conventions and discovery contract consumed by the separately deployed Okoscope Web UI.

## Requirements

### Requirement: API errors are consistent and correlated
Every versioned HTTP API error SHALL use a stable JSON envelope containing a machine-readable code, safe message, and request identifier, and every API response SHALL expose the same request identifier in a response header.

#### Scenario: Client supplies a valid request identifier
- **WHEN** a request includes a syntactically valid `X-Request-Id`
- **THEN** the server uses it for structured logs, the response header, and any error body

#### Scenario: Client omits or supplies an invalid identifier
- **WHEN** a request has no usable request identifier
- **THEN** the server generates a new opaque identifier and returns it without rejecting an otherwise valid request

#### Scenario: Internal error occurs
- **WHEN** an API handler encounters an internal database or implementation error
- **THEN** the response contains the correlation identifier and a safe generic message without internal details

### Requirement: Browser origins are explicitly configured
The server SHALL permit cross-origin API requests only from exact configured HTTP or HTTPS origins and MUST reject unsafe or malformed CORS configuration before serving traffic.

#### Scenario: Configured UI sends a preflight request
- **WHEN** the request Origin exactly matches the allowlist and requests an allowed method and header
- **THEN** the server returns the corresponding bounded CORS allow headers without enabling credentials

#### Scenario: Unknown origin sends a request
- **WHEN** the request Origin is absent from the allowlist
- **THEN** the server emits no CORS permission for that origin while normal API authentication remains enforced

#### Scenario: No origins are configured
- **WHEN** the server starts with an empty CORS allowlist
- **THEN** cross-origin access is disabled and same-origin or non-browser clients continue to work

#### Scenario: Wildcard or malformed origin is configured
- **WHEN** an origin contains a wildcard, credentials, path, query, fragment, unsupported scheme, or `null`
- **THEN** configuration validation fails before the server becomes ready

### Requirement: UI-facing API conventions are cache-safe and bounded
Protected read APIs SHALL return JSON, SHALL express timestamps as RFC 3339 UTC values, SHALL disable shared response caching, and SHALL enforce bounded deterministic cursor pagination on collection endpoints.

#### Scenario: Browser reads tenant data
- **WHEN** an authenticated client receives a protected navigation or observability response
- **THEN** the response uses JSON, includes `Cache-Control: no-store`, and contains no server-local timezone timestamps

#### Scenario: Page limit is omitted or excessive
- **WHEN** a list request omits a limit or requests more than the supported maximum
- **THEN** the server applies the documented default or returns a stable validation error without an unbounded query

### Requirement: OpenAPI describes the UI contract
The repository SHALL publish a valid OpenAPI 3.1 document that describes bearer authentication, shared schemas, pagination, idempotent commands, errors, and every endpoint required by the separate Web UI, including first-seen runtime-group investigation, lifecycle management, notification operations, Project notification worker health, and notification delivery recovery.

#### Scenario: Contract is validated in CI
- **WHEN** repository checks run
- **THEN** OpenAPI syntax, unique operation identifiers, documented security, concrete request and response schemas, idempotency headers, standard conflicts, and implementation route coverage are validated

#### Scenario: UI generates a client
- **WHEN** the separate UI consumes the published document
- **THEN** tenant navigation, runtime groups, group occurrences, lifecycle commands, releases and diffs, webhook destinations, delivery history, notification health, retry, cancel, bulk recovery, and recovery audit have stable request and response schemas rather than unbounded generic objects

#### Scenario: UI lists runtime groups
- **WHEN** the generated client requests an Application's groups
- **THEN** typed parameters cover event kind, lifecycle status, release, first-seen and last-seen time bounds, cursor, and limit

#### Scenario: UI investigates and updates a group
- **WHEN** the generated client reads a group, paginates its occurrences, or performs a lifecycle command
- **THEN** typed schemas expose discovery, lifecycle, notification, raw occurrence, request-ID, and cursor data required by the interface

#### Scenario: UI reads notification health
- **WHEN** the generated client requests an owned Project's notification health
- **THEN** a concrete schema exposes the bounded state enum, activation flag, enabled destination count, queue counts, oldest due age, and observation timestamp with no secret-bearing or unbounded fields

#### Scenario: UI executes a recovery command
- **WHEN** the generated client retries, cancels, or bulk-retries owned delivery work
- **THEN** typed request, idempotency, success, eligibility, conflict, and operation-summary schemas provide all data needed for explicit confirmation and correlated errors

### Requirement: Build compatibility is publicly inspectable
The server SHALL expose an unauthenticated build-info endpoint containing only service version, Git commit, API version, and required database migration.

#### Scenario: Deployed UI checks compatibility
- **WHEN** a client requests build info
- **THEN** it can determine the API version and build revision without presenting a credential

#### Scenario: Local build has no commit metadata
- **WHEN** Git commit metadata was not injected at build time
- **THEN** the endpoint returns a deterministic `unknown` value rather than failing startup

#### Scenario: Build info is inspected for secrets
- **WHEN** an unauthenticated caller retrieves build info
- **THEN** the response contains no credentials, connection strings, host identity, or dependency inventory

### Requirement: API foundation is observable
The server SHALL expose bounded metrics and structured logs for navigation requests, API errors by stable category, CORS denials, and request latency without labeling metrics by request ID, tenant ID, or raw URL.

#### Scenario: Operator investigates browser API failures
- **WHEN** navigation or CORS requests fail
- **THEN** aggregate metrics identify the route class and outcome while correlated logs provide the request identifier

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
