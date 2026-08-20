## ADDED Requirements

### Requirement: Release comparison includes observed TCP listeners
Release runtime comparison SHALL classify deterministic `network.listen` groups as new, disappeared, or unchanged using the existing trusted release attribution, selected operational scope, observation window, and unknown-evidence semantics.

#### Scenario: New release opens a listener
- **WHEN** a listener group is observed for the comparison release but not the baseline within trustworthy selected evidence
- **THEN** the release diff classifies that listener behavior as new

#### Scenario: Baseline listener is no longer observed
- **WHEN** a baseline listener group has no occurrence in the comparison release and the selected comparison evidence is sufficient
- **THEN** the release diff classifies it as disappeared rather than unknown

#### Scenario: Only accepted traffic changes
- **WHEN** the listener behavior is unchanged but accepted-connection counts or remote endpoints differ between releases
- **THEN** traffic differences do not create or remove listener behavioral identity in the release diff
