## ADDED Requirements

### Requirement: Typed plaintext DNS evidence
The system SHALL represent supported plaintext DNS queries and responses as typed `network.dns.query` and `network.dns.response` events containing trusted workload and process attribution, direction, transport, canonical question name and type, and for responses a response code, bounded canonical address answers, bounded CNAME evidence, and effective TTL.

#### Scenario: Selected workload resolves an address
- **WHEN** a selected process exchanges a valid plaintext UDP or TCP DNS message for an `A` or `AAAA` question
- **THEN** the agent emits validated typed query and response evidence without serializing the full packet

#### Scenario: Resolution has no address answer
- **WHEN** a valid response is NXDOMAIN, refused, truncated, or contains no supported address answer
- **THEN** the response event preserves the bounded question and response semantics without inventing an address mapping

### Requirement: DNS parsing is privacy bounded
DNS observation MUST inspect only traffic identified as plaintext DNS on port 53 and MUST NOT retain full packets, unrelated application payloads, EDNS option bodies, unsupported resource data, source ephemeral ports, process environment variables, unrestricted arguments, or encrypted DoH/DoT contents.

#### Scenario: DNS message contains unrelated resource data
- **WHEN** a supported response also contains TXT, MX, SRV, EDNS, or other non-address data
- **THEN** the agent discards that data and retains only the allowed bounded DNS fields

#### Scenario: Workload uses encrypted DNS
- **WHEN** resolution occurs through DoH, DoT, DNSCrypt, or another encrypted transport
- **THEN** the system emits no decoded DNS name and communicates that encrypted DNS is outside the observation boundary

### Requirement: DNS-to-connection correlation is qualified and bounded
The agent SHALL correlate responses to queries and recent address answers to `network.connect` occurrences using bounded workload-scoped state, clamped DNS TTLs, exact canonical IP matches, and explicit `observed_recently` confidence without changing connection group identity.

#### Scenario: Connection follows an observed answer
- **WHEN** a selected workload connects to an exact address from a non-expired observed DNS answer
- **THEN** the occurrence may include a bounded immutable DNS context with the observed name, evidence time, expiry, and `observed_recently` confidence

#### Scenario: Address has ambiguous names
- **WHEN** multiple non-expired observed names map to the same destination address in the trusted workload scope
- **THEN** the context retains a bounded set and marks the evidence as ambiguous rather than selecting a preferred name

#### Scenario: No trustworthy evidence exists
- **WHEN** the answer is expired, cached outside the observation window, unmatched, malformed, encrypted, or belongs to another workload
- **THEN** the connection remains IP-only and the system does not guess or reverse-resolve a name

### Requirement: Deployed DNS acceptance is reproducible
The release SHALL provide an operator-readable acceptance flow covering UDP and TCP DNS, IPv4 and IPv6 answers, response failures, CNAMEs, ambiguity, caching, malformed input, encrypted DNS limitations, replay, and selected/unselected workload isolation.

#### Scenario: Operator runs controlled DNS acceptance
- **WHEN** DNS observation is enabled only for the documented fixture workload
- **THEN** expected evidence is traced through capture, correlation, storage, grouping, APIs, notifications, and Web UI with bounded counters, cardinality, privacy assertions, and rollback evidence
