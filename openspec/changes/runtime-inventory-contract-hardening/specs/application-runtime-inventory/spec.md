## ADDED Requirements

### Requirement: Inventory summary follows the active operational scope
The inventory summary API SHALL accept release, cluster, namespace, workload kind, workload name, container name, observation-window, and bounded semantic-search filters with the same validation and matching semantics as the inventory list, and SHALL return aggregate totals for all supported inventory kinds within that selected scope.

#### Scenario: User filters to one deployment scope
- **WHEN** an authenticated user requests summary with release and Kubernetes scope filters
- **THEN** every returned item and occurrence count is derived only from inventory evidence matching all supplied filters

#### Scenario: User filters by observation window and search
- **WHEN** the user supplies a valid observation window and allowlisted semantic search
- **THEN** summary time bounds and per-kind counts describe the same matching item set as an inventory list request with those filters and no kind restriction

#### Scenario: User requests summary without filters
- **WHEN** no optional scope filter is supplied
- **THEN** summary retains the existing Application-wide aggregate behavior

#### Scenario: Summary filter is invalid
- **WHEN** a caller supplies an unsupported release scope, an invalid time range, or search outside documented bounds
- **THEN** the server returns the standard correlated validation error without returning partial aggregates or cross-tenant information

### Requirement: Deployment filter facets are discoverable and bounded
The server SHALL expose authenticated, tenant-safe, searchable, cursor-paginated facet collections for cluster, namespace, workload kind, workload name, and container name under an owned Application runtime inventory, with a maximum page size of 200 and deterministic ordering.

#### Scenario: User requests a cluster facet page
- **WHEN** an authenticated user requests cluster options for an owned Application
- **THEN** the server returns bounded UUID values with trusted cluster labels, exact matching item and occurrence counts, and an opaque next cursor

#### Scenario: User requests a textual facet page
- **WHEN** the user requests namespace, workload kind, workload name, or container name options
- **THEN** the server returns bounded inert canonical values and labels with exact matching counts in deterministic order

#### Scenario: User searches facet options
- **WHEN** the caller supplies a bounded facet-option search string
- **THEN** the server searches only the requested allowlisted facet value or trusted label and does not search unrestricted event JSON

#### Scenario: Caller requests an unsupported facet
- **WHEN** the facet path is not one of the documented closed values
- **THEN** the server returns a standard not-found or validation error without executing an unrestricted distinct query

### Requirement: Facets honor dependent scope while remaining replaceable
Each facet query SHALL apply kind, release, observation-window, semantic-search, and every deployment filter except the selected value of the requested facet, and SHALL bind its opaque cursor to the requested facet and normalized effective filter scope.

#### Scenario: Namespace options are requested with other filters
- **WHEN** cluster, namespace, workload, release, and time filters are active and the namespace facet is requested
- **THEN** namespace options ignore the selected namespace but honor cluster, workload, release, time, and every other effective filter

#### Scenario: Cursor is reused under another scope
- **WHEN** a facet cursor is supplied for another facet or after any effective filter changes
- **THEN** the server rejects it with the standard invalid-cursor error instead of skipping, duplicating, or leaking options

#### Scenario: One item has sightings in multiple Pods
- **WHEN** an inventory item matches multiple joined sightings within the effective facet scope
- **THEN** the facet item count counts that item once and occurrence count is not inflated by overlapping joins

### Requirement: Evidence navigation hints remain within trusted scoped routes
Inventory item evidence navigation SHALL use the known release, sighting, group, and occurrence routes for the current Project, Application, and item; any evidence link strings returned by the server MUST be root-relative allowlisted paths for that exact scope and MUST NOT contain an external authority, query, fragment, traversal, or encoded path separator.

#### Scenario: Server returns item detail links
- **WHEN** an owned inventory item detail is returned
- **THEN** each evidence hint matches one documented child collection route using the exact requested Project, Application, and item identifiers

#### Scenario: Frontend follows evidence navigation
- **WHEN** the Web UI loads an evidence collection
- **THEN** it constructs the path from typed scoped route parameters or validates a returned hint against the same relative allowlist before fetching

#### Scenario: Evidence-like value is untrusted
- **WHEN** observed semantic or Kubernetes text resembles a URL, route, traversal sequence, or encoded separator
- **THEN** it remains inert display evidence and cannot alter an evidence request target

### Requirement: Frontend contract fixtures cover complete inventory behavior
The backend repository SHALL provide bounded schema-aligned fixtures for summary, filtered summary, all four inventory kinds, item detail, release presence, sightings, contributing groups, occurrences, facet pages, empty and terminal pages, pagination, standard errors, and unsafe observed display values.

#### Scenario: Frontend tests success responses
- **WHEN** the separately developed Web UI consumes backend contract fixtures
- **THEN** fixtures provide complete typed examples for every inventory route and all `observed`, `not_observed`, and `unknown` release states

#### Scenario: Frontend tests failure responses
- **WHEN** the UI tests invalid cursor, unauthorized, not-found, and server-error behavior
- **THEN** fixtures provide the standard correlated error envelope for each state without credentials or sensitive payload contents

#### Scenario: Frontend tests inert rendering
- **WHEN** the UI renders unsafe-value fixtures
- **THEN** every semantic and Kubernetes display field can be verified as inert text in lists, details, facets, tooltips, and error-adjacent views

### Requirement: Frontend snapshot synchronization is explicit and verifiable
The runtime inventory frontend handoff SHALL identify backend OpenAPI as source of truth and SHALL require the frontend-owned snapshot and generated schema types to be refreshed and verified before inventory UI implementation is accepted.

#### Scenario: Backend inventory contract changes
- **WHEN** scoped summary, facet, evidence-link, or fixture contracts are finalized
- **THEN** the handoff records the reviewed backend artifact and the frontend repository replaces its snapshot and regenerates `schema.d.ts`

#### Scenario: Generated frontend types drift
- **WHEN** frontend CI regenerates schema types from its synchronized snapshot
- **THEN** CI fails if generation leaves an uncommitted diff or required runtime inventory operations, filters, facet values, responses, or errors are absent
