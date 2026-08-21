## ADDED Requirements

### Requirement: Application inventory exposes file activity
Application runtime inventory SHALL expose file create, modify, delete, and rename identities with safe semantic summaries, aggregate occurrence metadata, release-scoped presence, and navigation to their runtime groups and raw occurrences.

#### Scenario: Application modifies a file
- **WHEN** an accepted `file.modify` occurrence is grouped for an Application
- **THEN** inventory exposes its operation, normalized path, process identity, lifecycle totals, and release presence

#### Scenario: Application renames a file
- **WHEN** an accepted `file.rename` occurrence is grouped for an Application
- **THEN** inventory exposes the bounded old and new paths and replacement identity without file contents or host paths

### Requirement: Release comparison includes file activity
Release runtime diff SHALL classify stable file activity group identities as new, unchanged, or disappeared using exact release-scoped occurrence summaries.

#### Scenario: File behavior appears in a release
- **WHEN** a file activity group is absent from the base release and present in the target release
- **THEN** release comparison classifies it as new and provides navigation to the same tenant-scoped group evidence

