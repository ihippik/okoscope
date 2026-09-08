# Changelog

All notable changes to Okoscope are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Okoscope is pre-1.0, so minor releases may contain documented breaking changes.

## [Unreleased]

### Added

- Open-source governance, maintainer lifecycle, contribution, support, security,
  release, architecture, roadmap, adopter, and Code of Conduct documentation.
- GitHub ownership rules, pull-request template, and structured forms for bug
  reports, feature proposals, and support questions.
- A CNCF Sandbox readiness matrix that separates existing evidence from work
  that still requires real community, adoption, security, or legal outcomes.
- Private vulnerability reporting and public cloud-native repository metadata.

### Changed

- Corrected Cargo repository metadata to reference the current public source
  repository.
- Expanded the README with project status and community entry points.

## [0.2.1] - 2026-09-08

### Changed

- Coordinated the server, agent, web, Helm chart, Cargo workspace, release
  metadata, and OpenAPI examples on version `0.2.1`.
- Updated installation and Helm documentation to recommend chart version
  `0.2.1`.
- Published the matching packaged `okoscope-agent` chart dependency.

## [0.2.0] - 2026-09-08

### Added

- First Git-tagged, coordinated Okoscope release.
- Linux and Kubernetes runtime observation with eBPF-backed process, lifecycle,
  file, network, DNS, and resource signals.
- Kubernetes workload attribution, authenticated bidirectional agent transport,
  PostgreSQL persistence, runtime grouping, inventory, release comparison, and
  retention policies.
- Tenant-scoped HTTP API with an authoritative OpenAPI contract, notification
  delivery and recovery, onboarding, and user authorization flows.
- OCI Helm charts for a standalone agent and a self-hosted control plane using
  an operator-owned PostgreSQL database.
- CI validation for Rust formatting, strict Clippy, userspace tests, PostgreSQL
  migrations, Helm contracts, and Kubernetes manifests.

All repository history before the first tagged release is represented by
`0.2.0`; earlier packaged chart versions were development artifacts rather than
Git-tagged project releases.

[Unreleased]: https://github.com/ihippik/okoscope/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/ihippik/okoscope/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ihippik/okoscope/releases/tag/v0.2.0
