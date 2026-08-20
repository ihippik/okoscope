## ADDED Requirements

### Requirement: Runtime diff summary covers the complete comparison
The server SHALL expose `GET /api/v1/projects/{project_id}/applications/{application_id}/releases/{target_id}/runtime-diff/summary` for an owned target release. It SHALL use the supplied owned `baseline_id` or the same stable backend-selected baseline as Runtime Diff, reuse the existing typed identity comparison rules, and compute totals from the complete comparison independently of the Runtime Diff cursor.

#### Scenario: Explicit or default baseline exists
- **WHEN** an authenticated caller requests a summary with a valid explicit baseline or omits it when a previous release exists
- **THEN** the response identifies that baseline and the target, target.id equals `target_id`, and all aggregate fields cover their complete comparison

#### Scenario: Baseline does not exist for the target
- **WHEN** no explicit baseline is supplied and backend baseline selection finds none
- **THEN** the response contains `baseline: null`, the owned target, zero totals, empty `classifications`, and empty `largest_changes`

#### Scenario: Release scope is invalid
- **WHEN** the Project, Application, target, or explicit baseline does not belong to the authenticated tenant and exact URL scope
- **THEN** the server returns the standard correlated 401 or tenant-safe 404 error and no release or runtime data

### Requirement: Runtime diff classifications are exact
The summary SHALL return exact `new`, `disappeared`, and `unchanged` item counts over the complete identity comparison, and their sum MUST equal `total_item_count`. `unchanged` SHALL mean the typed identity occurs in both releases regardless of whether occurrence counts are equal.

#### Scenario: Identity presence differs by release
- **WHEN** identities occur only in target, only in baseline, or in both
- **THEN** the server counts them respectively as `new`, `disappeared`, or `unchanged` exactly once

#### Scenario: Unchanged identity count changes
- **WHEN** the same identity occurs in both releases with different occurrence counts
- **THEN** it remains classified as `unchanged`

### Requirement: Largest runtime changes are bounded and deterministic
The summary SHALL accept `limit` with default 5, minimum 1, and maximum 10 and return at most that many `largest_changes`. It MUST compute `occurrence_delta` as target minus baseline occurrence count, use zero baseline count for `new`, use zero target count for `disappeared`, and order by absolute delta descending with immutable `group_id` ascending as the stable tie-breaker.

#### Scenario: Positive, negative, and zero deltas exist
- **WHEN** the complete comparison contains increases, decreases, and no-count-change identities
- **THEN** each returned delta has the correct sign and value and selection is based on its absolute magnitude

#### Scenario: Absolute deltas are tied
- **WHEN** two candidate groups have equal absolute occurrence deltas
- **THEN** they are returned in ascending immutable group ID order across repeated requests over unchanged data

#### Scenario: Limit is outside bounds
- **WHEN** limit is 0 or 11
- **THEN** the server returns the standard correlated 400 validation error without executing an unbounded comparison

### Requirement: Runtime diff summary is bounded, safe, and verifiable
The implementation SHALL aggregate in the database and retrieve bounded top-N results without loading every comparison group into application memory. Semantic summaries SHALL be bounded, event-kind appropriate, and inert text, and OpenAPI plus automated tests SHALL cover ownership, baseline behavior, multi-page-sized comparisons, classifications, deltas, ordering, limits, exact totals, standard errors, and hostile observed strings.

#### Scenario: High-cardinality diff summary is measured
- **WHEN** the performance workload compares releases with many groups
- **THEN** the recorded evidence includes dataset volume, p50/p95/p99 latency, maximum response size, query plan, used indexes, accepted top-N maximum of 10, and discovered limitations

