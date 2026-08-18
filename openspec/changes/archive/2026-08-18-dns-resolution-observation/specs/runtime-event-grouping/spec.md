## ADDED Requirements

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
