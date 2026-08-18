# runtime-event-grouping Specification

## Purpose

Define how accepted runtime events are grouped into durable, tenant-scoped runtime findings with deterministic identity, occurrence lifecycle, first-seen outbox work, query APIs, and safe historical backfill.

## Requirements

### Requirement: Accepted runtime events are assigned to deterministic groups
The server SHALL assign every newly persisted supported runtime event to exactly one group using a deterministic, versioned fingerprint derived from server-trusted tenant and workload scope plus the event's semantic identity.

#### Scenario: Equivalent process executions are ingested
- **WHEN** the same executable is observed more than once for the same Organization, Project, Application, Cluster, namespace, workload kind, and workload name
- **THEN** all observations are assigned to the same `process.exec` group despite differing event, Pod, container, PID, or workload UID values

#### Scenario: Equivalent syscalls are ingested
- **WHEN** the same canonical syscall is invoked by the same process command in the same trusted grouping scope
- **THEN** all observations are assigned to the same syscall group

#### Scenario: Semantic identity differs
- **WHEN** event kind, executable, process command, syscall name, Application, Cluster, namespace, or top-level workload identity differs
- **THEN** the observations are assigned to different groups

### Requirement: Fingerprint evolution is explicit
Each group SHALL store a fingerprint version, and the server MUST use an explicit canonical encoding and cryptographic digest so implementation or serialization changes cannot silently alter grouping behavior.

#### Scenario: Current fingerprint algorithm is used
- **WHEN** the server calculates a fingerprint
- **THEN** it stores both the digest and the configured fingerprint version and enforces uniqueness within the trusted grouping scope and version

#### Scenario: Fingerprint algorithm changes in a later release
- **WHEN** a new canonicalization algorithm is introduced
- **THEN** it uses a new fingerprint version and does not silently merge its results into groups created by another version

### Requirement: Group lifecycle summarizes occurrences
Every runtime-event group SHALL track immutable discovery metadata (`first_seen` and `first_seen_event_id`), monotonic `last_seen`, an exact `occurrence_count`, an immutable representative event, event kind, semantic summary, and an operator-managed status whose initial value is `open`; when an occurrence has a resolved release, the server SHALL also update an exact release-scoped summary without changing group identity. Status transitions SHALL record their server timestamp and authenticated actor, and new occurrences SHALL NOT implicitly reopen a group.

#### Scenario: First occurrence creates a group
- **WHEN** an event fingerprint has no existing group
- **THEN** the server creates an `open` group with `first_seen` and `last_seen` equal to the event observation time, `occurrence_count` equal to one, and that event as both its representative and first-seen event

#### Scenario: Later occurrence updates a group
- **WHEN** a newly persisted event belongs to an existing group
- **THEN** the server preserves the first-seen identity and representative event, advances `last_seen` when appropriate, and increments `occurrence_count` exactly once

#### Scenario: Delayed older event arrives
- **WHEN** an event is received after the group exists but its observation time is earlier than the stored `first_seen`
- **THEN** the server adjusts `first_seen` and `first_seen_event_id` to the earlier observation without replacing the representative event or decreasing `last_seen`

#### Scenario: Attributed occurrence updates release summary
- **WHEN** a distinct group occurrence has a server-resolved release
- **THEN** the server updates that group's summary for the release exactly once while preserving the cross-release group fingerprint and aggregate lifecycle

#### Scenario: Resolved behavior occurs again
- **WHEN** a new occurrence belongs to a resolved group
- **THEN** occurrence fields are updated while the group remains resolved until an operator explicitly reopens it

### Requirement: Group counting is idempotent and concurrency-safe
The database SHALL record unique event-to-group membership, including optional server-resolved release attribution, and MUST NOT inflate aggregate or release-scoped `occurrence_count` values when agents retry events or concurrent transactions observe the same fingerprint.

#### Scenario: Agent retries an accepted event
- **WHEN** the same agent identity and event ID are ingested again
- **THEN** no new membership is created and the aggregate and release-scoped group counts remain unchanged

#### Scenario: Concurrent first occurrences share a fingerprint
- **WHEN** concurrent transactions ingest distinct events with the same previously unseen fingerprint
- **THEN** exactly one group exists and its aggregate and applicable release-scoped occurrence counts equal the number of distinct accepted events

### Requirement: First-seen work is recorded transactionally
The server SHALL create exactly one durable outbox record for a newly created group in the same database transaction as the raw event, group, and membership changes.

#### Scenario: New group transaction commits
- **WHEN** the first event for a fingerprint is committed
- **THEN** one pending `runtime_group.first_seen` outbox record referencing the group is committed atomically

#### Scenario: Existing group receives an occurrence
- **WHEN** another event is assigned to an existing group
- **THEN** no additional first-seen outbox record is created

#### Scenario: Ingestion transaction rolls back
- **WHEN** raw event, grouping, membership, or outbox persistence fails
- **THEN** none of those changes are committed and the batch is not acknowledged as committed

### Requirement: Raw events remain available
Grouping SHALL augment rather than replace raw runtime-event storage, and each membership SHALL reference an existing tenant-consistent raw event and group.

#### Scenario: Operator investigates a group
- **WHEN** a group is retrieved
- **THEN** its representative event and recent raw occurrences remain queryable with their original Kubernetes attribution and payload

### Requirement: Group queries are tenant scoped
The server SHALL expose versioned read endpoints for listing groups, retrieving one group, and cursor-paginating its raw occurrences, and SHALL derive Organization scope from an authenticated API principal rather than trust a tenant identifier supplied by the caller.

#### Scenario: Authorized principal lists groups
- **WHEN** an authenticated principal requests groups for an owned Project and Application with pagination and optional event kind, lifecycle status, release, first-seen, or last-seen filters
- **THEN** the server returns only matching groups in that Organization using stable most-recent-first ordering

#### Scenario: Authorized principal retrieves a group
- **WHEN** an authenticated principal requests an owned group
- **THEN** the response includes aggregate fields, discovery metadata, lifecycle metadata, semantic summary, representative event, and notification summary

#### Scenario: Authorized principal lists occurrences
- **WHEN** an authenticated principal requests occurrences for an owned group with a bounded cursor page
- **THEN** the server returns original raw events in stable observation-time and event-ID order with Kubernetes and release attribution

#### Scenario: Principal references another organization
- **WHEN** an authenticated principal requests a Project, Application, group, or occurrence outside its Organization
- **THEN** the server returns no cross-tenant data

### Requirement: Operators manage group lifecycle explicitly
The server SHALL provide idempotent tenant-scoped commands to acknowledge, resolve, and reopen runtime groups, SHALL validate permitted transitions, and SHALL preserve complete occurrence and discovery history.

#### Scenario: Open group is acknowledged
- **WHEN** an authorized principal acknowledges an open group
- **THEN** its status becomes `acknowledged` and the transition actor and server timestamp are recorded

#### Scenario: Group is resolved
- **WHEN** an authorized principal resolves an open or acknowledged group
- **THEN** its status becomes `resolved` and the transition actor and server timestamp are recorded

#### Scenario: Group is reopened
- **WHEN** an authorized principal reopens an acknowledged or resolved group
- **THEN** its status becomes `open` without changing its first-seen metadata, representative event, or occurrence count

#### Scenario: Command is retried
- **WHEN** the requested group already has the target status
- **THEN** the command succeeds idempotently without creating a second logical transition

#### Scenario: Transition is invalid
- **WHEN** a command requests an unsupported status transition
- **THEN** the server rejects it with a stable validation error and leaves the group unchanged

### Requirement: Existing events can be backfilled safely
The system SHALL provide an explicit, restartable backfill operation that groups previously stored raw events using a selected fingerprint version without generating external notification delivery.

#### Scenario: Backfill processes existing raw events
- **WHEN** an operator runs backfill for an Organization and Project
- **THEN** missing memberships and aggregate counts are created idempotently in bounded batches

#### Scenario: Backfill encounters already grouped events
- **WHEN** a raw event already has membership for the selected fingerprint version
- **THEN** the operation skips it without incrementing the group count or duplicating outbox records

### Requirement: Deterministic outbound destination grouping
The server SHALL group `network.connect` events using the trusted tenant and workload scope, fingerprint version, event kind, normalized process command, address family, canonical destination IP, and destination port; connection outcome and errno MUST NOT change group identity.

#### Scenario: Repeated outcomes target the same endpoint
- **WHEN** the same selected workload and process command attempts the same destination with successful, in-progress, and failed outcomes
- **THEN** all idempotently accepted occurrences update one runtime group while retaining their individual outcomes

#### Scenario: Destination changes
- **WHEN** the destination address or port differs for an otherwise identical network attempt
- **THEN** the server assigns the event to a distinct deterministic runtime group

#### Scenario: Release attribution changes
- **WHEN** identical network behavior occurs in two resolved Application releases
- **THEN** group identity remains stable and exact release-scoped summaries support existing new, disappeared, and unchanged comparison

### Requirement: Safe network semantic summary
Each network group SHALL expose a bounded semantic summary containing process command, address family, canonical destination IP, and destination port, and first-seen notification materialization SHALL use only those safe fields plus existing group identity and tenant-scoped metadata.

#### Scenario: First network behavior is grouped
- **WHEN** the first accepted occurrence creates a `network.connect` group
- **THEN** exactly one first-seen outbox record is created with the safe semantic summary and no packet, DNS, HTTP, TLS, source-port, or socket-buffer content

### Requirement: Deterministic DNS behavior grouping
The server SHALL group DNS events using trusted tenant/workload scope, fingerprint version, event kind, normalized process command, canonical question name and type, and response code where applicable while keeping volatile answer sets and transaction identifiers out of group identity.

#### Scenario: Repeated resolution behavior occurs
- **WHEN** the same selected workload and process repeats an equivalent DNS question or response behavior
- **THEN** distinct accepted occurrences update one deterministic group with exact aggregate and release-scoped counts

#### Scenario: DNS semantic identity differs
- **WHEN** the question name, type, process command, response code, event kind, or trusted scope differs
- **THEN** the server assigns the event to a distinct group

### Requirement: Connection grouping remains IP first
Correlated DNS names MUST NOT alter `network.connect` fingerprint identity, and connection semantic summaries SHALL expose only a bounded qualified DNS context that cannot accumulate names across unrelated or expired occurrences.

#### Scenario: Same endpoint follows different names
- **WHEN** otherwise identical connections to one IP and port carry different valid recent DNS contexts
- **THEN** they remain in one connection group while each occurrence retains its own qualified evidence

#### Scenario: First-seen notification includes DNS evidence
- **WHEN** a first occurrence has valid bounded DNS context
- **THEN** notification materialization may include only the safe qualified context and existing group metadata without full packets or unbounded names
