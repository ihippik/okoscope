## ADDED Requirements

### Requirement: Applications expose a unified semantic runtime inventory
The server SHALL derive one tenant-scoped application inventory item for each supported semantic behavior independently of cluster, namespace, workload, Pod, container, and release identity, while preserving links to every contributing deployment-scoped runtime group and raw occurrence.

#### Scenario: Equivalent behavior occurs in multiple deployment scopes
- **WHEN** the same supported semantic behavior is observed for one Application in different clusters, namespaces, or workloads
- **THEN** the server represents it as one application inventory item with multiple contributing sightings and runtime groups

#### Scenario: Behavior belongs to another application
- **WHEN** equivalent semantic behavior is observed for a different Application
- **THEN** the server assigns it to a different inventory item and exposes no cross-Application evidence

### Requirement: Inventory identity is deterministic and versioned
The server MUST derive inventory identity with an explicit version, canonical length-delimited encoding, and cryptographic digest over the trusted Organization, Project, and Application scope, inventory kind, and kind-specific semantic fields.

#### Scenario: Equivalent event is retried
- **WHEN** the same accepted event or an equivalent event is processed more than once under one identity version
- **THEN** its canonical inventory identity remains stable and no duplicate item is created

#### Scenario: Identity rules evolve
- **WHEN** a later release changes canonical inventory identity rules
- **THEN** it uses a new identity version and does not silently merge items created under another version

### Requirement: Executed processes are inventoried by executable
The server SHALL map each accepted `process.exec` event to a `process` inventory item identified by its canonical executable within the trusted Application scope; parent command, Pod, container, workload, and release SHALL remain evidence rather than item identity.

#### Scenario: Executable runs in multiple Pods
- **WHEN** the same executable is observed in multiple Pods or containers of one Application
- **THEN** one process item summarizes all idempotently accepted observations and their distinct sightings

#### Scenario: Executable changes
- **WHEN** two process execution events have different canonical executables
- **THEN** the server represents them as different process inventory items

### Requirement: Outbound destinations are inventoried by process and endpoint
The server SHALL map each accepted `network.connect` event to a `destination` inventory item identified by canonical process command, address family, destination IP address, and destination port; outcome, errno, and correlated DNS context MUST NOT change item identity.

#### Scenario: Connection outcomes differ
- **WHEN** one process reports successful, in-progress, and failed attempts to the same canonical endpoint
- **THEN** one destination item summarizes the attempts while each occurrence retains its outcome and bounded errno evidence

#### Scenario: Endpoint differs
- **WHEN** the destination address, address family, port, or process command differs
- **THEN** the server represents the behavior as a different destination inventory item

### Requirement: DNS behavior is inventoried as unified domains
The server SHALL map supported DNS query and response events to a `domain` inventory item identified by canonical process command, question name, and query type, while keeping event direction, transport, response code, answers, CNAME evidence, and TTL out of item identity.

#### Scenario: Query receives a response
- **WHEN** a process emits a supported DNS query and response for the same canonical name and query type
- **THEN** both events contribute to one domain inventory item and remain separately inspectable evidence

#### Scenario: Response semantics vary
- **WHEN** repeated responses for one domain item have different response codes, addresses, CNAMEs, or TTLs
- **THEN** the domain item identity remains stable and the varying response evidence remains available in bounded detail

### Requirement: Syscalls are inventoried by process and canonical name
The server SHALL map each accepted `syscall` event to a `syscall` inventory item identified by canonical process command and canonical syscall name within the trusted Application scope.

#### Scenario: Same process invokes a syscall across scopes
- **WHEN** the same canonical process command invokes the same syscall in multiple deployment scopes
- **THEN** one syscall inventory item summarizes the accepted occurrences and sightings

#### Scenario: Process or syscall differs
- **WHEN** the canonical process command or syscall name differs
- **THEN** the server represents the behavior as a different syscall inventory item

### Requirement: Inventory lifecycle summaries are exact and idempotent
Every inventory item SHALL maintain immutable semantic identity, earliest observed time, latest observed time, and an exact accepted occurrence count, and SHALL update those fields exactly once per source event under retries, concurrent ingestion, and delayed delivery.

#### Scenario: First occurrence is projected
- **WHEN** the first accepted event for an inventory identity is committed
- **THEN** the server creates an item with equal first and last observed times and an occurrence count of one

#### Scenario: Distinct later occurrence is projected
- **WHEN** another distinct accepted event maps to the item
- **THEN** the count increments exactly once and the time bounds expand as required

#### Scenario: Delayed older occurrence is projected
- **WHEN** a distinct event arrives after item creation with an earlier observation time
- **THEN** first observed moves to the earlier time without decreasing last observed or duplicating the occurrence

#### Scenario: Agent retries an event
- **WHEN** an already projected source event is retried
- **THEN** item, release, and sighting counts remain unchanged

### Requirement: Inventory summarizes bounded Kubernetes sightings
The server SHALL maintain tenant-consistent inventory sightings across cluster, namespace, top-level workload, Pod UID, and container name and SHALL expose bounded distinct counts in list responses with separately paginated detail.

#### Scenario: Item appears in another Pod
- **WHEN** an inventory item is observed in a previously unseen Pod of the same trusted Application
- **THEN** its distinct Pod count and paginated sighting evidence include that Pod without creating another inventory item

#### Scenario: Client requests inventory list
- **WHEN** a caller lists inventory items
- **THEN** the response returns bounded facet counts and does not embed an unbounded collection of Pods, containers, groups, releases, or events

### Requirement: Inventory reports evidence-qualified release presence
The server SHALL classify an inventory item's relation to an owned release as `observed`, `not_observed`, or `unknown`, SHALL include supporting attributed evidence counts and time bounds, and MUST NOT describe observational absence as proof that behavior cannot occur.

#### Scenario: Item has attributed release evidence
- **WHEN** at least one trusted occurrence for an item is attributed to the release
- **THEN** the release presence is `observed` with exact occurrence and first and last observed fields

#### Scenario: Release has other attributed evidence
- **WHEN** the selected release and scope contain trusted attributed runtime evidence but none maps to the item
- **THEN** the release presence is `not_observed` and communicates that the classification is limited to available evidence

#### Scenario: Release lacks trustworthy evidence
- **WHEN** the server has no trusted attributed evidence with which to evaluate the item for the selected release and scope
- **THEN** the release presence is `unknown` rather than `not_observed`

### Requirement: Inventory APIs are tenant-safe, filterable, and paginated
The server SHALL expose authenticated versioned APIs under the Project and Application hierarchy for inventory summary, item listing, item detail, release presence, deployment sightings, contributing runtime groups, and raw occurrences, deriving Organization scope from the authenticated principal and using stable bounded cursor pagination.

#### Scenario: Principal lists owned application inventory
- **WHEN** an authenticated principal requests an owned Application with supported kind, release, cluster, namespace, workload, container, observation-window, or semantic-search filters
- **THEN** the server returns a deterministic bounded page containing only matching tenant-scoped inventory items

#### Scenario: Principal investigates an item
- **WHEN** an authenticated principal retrieves an owned item and its bounded detail collections
- **THEN** the server returns safe semantic identity, lifecycle summary, release presence, deployment sightings, contributing groups, and navigable raw evidence

#### Scenario: Principal references another tenant
- **WHEN** a principal references a Project, Application, release, item, group, or occurrence outside its Organization
- **THEN** the server returns no cross-tenant inventory or existence information

#### Scenario: Search targets unsupported data
- **WHEN** a caller attempts to search unrestricted event JSON or a semantic field outside the allowlist
- **THEN** the server rejects or safely ignores the unsupported filter without scanning or returning sensitive payload data

### Requirement: Web UI presents one evidence-backed application inventory
The Web UI SHALL provide an Application runtime inventory with an aggregate overview, separate process, destination, domain, and syscall views, explicit active scope and filters, and item detail that links summarized behavior to release presence, Kubernetes sightings, runtime groups, and raw occurrences.

#### Scenario: User opens an application inventory
- **WHEN** an authenticated user opens an owned Application
- **THEN** the UI shows bounded counts for all supported inventory kinds, the active observation and deployment scope, and a paginated inventory view without requiring manual reconciliation of duplicate workload groups

#### Scenario: User investigates an inventory item
- **WHEN** the user selects a process, destination, domain, or syscall item
- **THEN** the UI shows its safe semantic identity, first and last observation, occurrence count, release evidence, deployment sightings, and links to contributing runtime evidence

#### Scenario: Release evidence is incomplete
- **WHEN** an item's release presence is `not_observed` or `unknown`
- **THEN** the UI uses evidence-qualified wording and does not present the state as proof that the behavior cannot occur

#### Scenario: Inventory contains untrusted display text
- **WHEN** semantic or Kubernetes fields contain markup-like content
- **THEN** the UI renders them as inert text and does not execute links, markup, or scripts derived from observed data

### Requirement: Inventory projection is backfillable and observable
The system SHALL provide a bounded restartable backfill using the same identity and projection rules as live ingestion, SHALL suppress external first-seen notifications during backfill, and SHALL expose metrics, structured logs, progress, freshness, skip reasons, and reconciliation outcomes without sensitive event contents.

#### Scenario: Existing events are backfilled
- **WHEN** an operator runs inventory backfill for a selected tenant scope and identity version
- **THEN** missing items, memberships, release summaries, group links, and sightings are created idempotently in bounded batches

#### Scenario: Backfill resumes after interruption
- **WHEN** an interrupted backfill is restarted
- **THEN** existing memberships are skipped and exact aggregate counts are not inflated

#### Scenario: Operator checks projection correctness
- **WHEN** an operator reconciles an Application inventory with source events and group memberships
- **THEN** bounded diagnostics report freshness and count or time-bound mismatches without exposing event payload contents
