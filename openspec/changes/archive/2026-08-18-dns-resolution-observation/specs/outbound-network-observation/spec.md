## ADDED Requirements

### Requirement: Connection investigation may include observed DNS context
Authenticated users SHALL see optional bounded DNS evidence on a `network.connect` occurrence only when the selected workload recently observed an exact address answer, and the API and UI MUST distinguish evidence from canonical destination identity.

#### Scenario: Recent DNS evidence exists
- **WHEN** an owned connection occurrence has valid correlated DNS context
- **THEN** investigation shows inert canonical names, evidence age/expiry, confidence, and ambiguity while retaining IP and port as the destination

#### Scenario: DNS context is unavailable
- **WHEN** resolution was cached, expired, encrypted, unmatched, or unobserved
- **THEN** investigation remains fully usable with IP-only evidence and explains that absence without performing reverse DNS
