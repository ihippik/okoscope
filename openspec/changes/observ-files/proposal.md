## Why

Okoscope cannot currently explain which files a selected Kubernetes workload creates, modifies, deletes, or renames; its generic syscall events carry only a syscall name and cannot reliably identify a path or successful filesystem outcome. Operators need bounded, privacy-conscious file activity evidence for runtime inventory and release comparison without capturing file contents or flooding storage with every write call.

## What Changes

- Add an opt-in file activity observation capability for successful create, aggregated modify, delete, and rename operations on regular files.
- Require at least one absolute `includePaths` entry and support higher-priority `excludePaths` entries, matched on path-component boundaries against absolute pathname arguments supplied by the process.
- Introduce an explicitly experimental syscall-path profile: retain only bounded normalized absolute pathname arguments, correlate descriptor writes with successful tracked opens, and count unsupported relative, unresolved, or truncated paths without claiming filesystem-resolved identity.
- Aggregate repeated modifications to the same workload/container/mount/inode over a fixed five-second code-defined window, flushing pending modification evidence before rename or delete.
- Treat rename as a distinct event with old and new paths when both are observable; translate scope crossings to create or delete without exposing excluded paths.
- Extend the typed event protocol, ingestion, grouping, inventory, APIs, metrics, documentation, and acceptance coverage for file activity while never capturing file contents or written bytes.

## Capabilities

### New Capabilities

- `file-activity-observation`: Configuration, kernel observation, path filtering, bounded aggregation, attribution, typed delivery, and operational visibility for regular-file activity.

### Modified Capabilities

- `runtime-event-storage`: Persist, validate, retain, and query typed file activity events.
- `runtime-event-grouping`: Group equivalent file activity into stable runtime evidence without using occurrence-only fields.
- `application-runtime-inventory`: Expose file activity identities and occurrences through application runtime inventory and release-aware views.
- `agent-server-session`: Advertise and report the health of the versioned file activity capability and its loss counters.
- `workload-event-observation`: Apply workload selection, privacy rules, capability readiness, and global safety limits to file activity observation.

## Impact

- Agent configuration, capability reporting, eBPF programs/maps, userspace decoding, aggregation, path filtering, counters, and workload attribution.
- Shared event model and kernel/userspace ABI plus protobuf wire contracts.
- Server ingestion, persistence migrations, grouping, runtime inventory, release diff behavior, metrics, and OpenAPI responses.
- Agent tracepoint capability checks for the selected syscall-path profile; unsupported required syscall coverage remains fail-closed.
- Kubernetes examples, operational documentation, fixtures, and bounded selected/unselected workload acceptance tests.
