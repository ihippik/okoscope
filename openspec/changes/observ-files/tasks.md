## 1. Contract and configuration

- [x] 1.1 Add strict default-disabled `observation.files` configuration with operation validation, required normalized absolute `includePaths`, optional `excludePaths`, and component-boundary matching tests.
- [x] 1.2 Define shared file activity operations, bounded path types, operation-specific payload invariants, and the five-second `FILE_MODIFY_AGGREGATION_WINDOW` constant with unit tests.
- [x] 1.3 Extend protobuf messages and capability constants for backward-compatible `file.activity/v1` transport and regenerate checked-in protocol bindings.
- [x] 1.4 Add protocol round-trip and malformed-payload tests for create, modify, delete, rename, replacement identity, path bounds, normalization, and NUL rejection.

## 2. Kernel capability investigation and ABI

- [x] 2.1 Build a read-only probe matrix for claimed 5.15, 6.1, and 6.8 kernels covering syscall entry/exit formats, bounded user-path reads, success returns, and descriptor correlation requirements.
- [x] 2.2 Select and document the exact open/write/truncate/unlink/rename/close syscall profile, including relative-path, canonicalization, creation-certainty, and memory-mapped writeback gaps.
- [x] 2.3 Define the fixed-size kernel/userspace ABI record, bounded correlation/filter/counter contracts, descriptor generation, path data, command, and cgroup fields with compile-time layout assertions.
- [x] 2.4 Add userspace ABI decoders and fixtures that reject wrong sizes, unknown operations, incomplete or invalid paths, and invalid flags.

## 3. eBPF file observation

- [x] 3.1 Implement success-aware certain-create (`O_CREAT|O_EXCL`) and pathname/fd truncate probes with bounded entry/return correlation under the experimental syscall-path profile.
- [x] 3.2 Implement successful write probes correlated to a bounded `(TGID, fd, generation)` path retained from a successful absolute-path open without copying content or byte buffers.
- [x] 3.3 Implement success-aware unlink probes that retain the old absolute syscall path until return and exclude `AT_REMOVEDIR`.
- [x] 3.4 Implement success-aware rename probes that retain complete ordered old/new syscall paths and report replacement identity only when flags prove it.
- [x] 3.5 Reject unsupported path shapes in kernel space and document why variable configured prefix filters remain in immediate bounded userspace validation for the 5.15 syscall-path profile.
- [x] 3.6 Add monotonic kernel counters for correlation capacity/miss, path read/relative/invalid/oversize, descriptor misses, filtering, and ring output loss.

## 4. Loader, readiness, and attribution

- [x] 4.1 Extend the observer loader to validate and atomically attach the complete configured syscall tracepoint profile, clean up partial attachment, and fail readiness when required coverage is unavailable.
- [x] 4.2 Expose `file.activity.syscall-path/v1` only after the complete syscall tracepoint profile attaches successfully; keep validated variable prefix filters in authoritative userspace matching.
- [x] 4.3 Route decoded file records through existing cgroup/container/Kubernetes selection and reject uncertain or unselected attribution with reason counters.
- [x] 4.4 Repeat authoritative normalization and include/exclude filtering in userspace before any path enters delivery, diagnostics, or persistence.

## 5. Modification aggregation and event production

- [x] 5.1 Implement bounded userspace aggregation keyed by workload UID, container identity, TGID, descriptor, and descriptor generation with a fixed five-second expiry and last reported path/process metadata.
- [x] 5.2 Flush pending modification before rename or delete and verify deterministic causal ordering across repeated writes and structural operations.
- [x] 5.3 Implement bounded shutdown/container-expiry handling and capacity/drop policy with dedicated monotonic counters.
- [x] 5.4 Translate rename scope crossings into safe create/delete events, emit rename only when both sides are observable, and prevent excluded paths from entering delivery.
- [x] 5.5 Feed typed file events through existing global rate, queue, batch, retry, replay, and acknowledgement mechanisms.

## 6. Server persistence and ingestion

- [x] 6.1 Add an ordered PostgreSQL migration and repository mappings needed to store and decode canonical file activity payloads on empty and upgraded databases.
- [x] 6.2 Extend server ingestion validation for operation-specific fields, complete normalized bounded paths, replacement identity, capability compatibility, tenant ownership, and atomic batch rejection.
- [x] 6.3 Add persistence and ingestion tests for valid events, malformed paths, replay idempotency, cross-tenant rejection, release attribution, and rollback behavior.

## 7. Grouping, inventory, and APIs

- [x] 7.1 Add a new explicit fingerprint version or compatible canonical encoding for file event kind, normalized process command, path identity, and ordered rename identity with deterministic test vectors.
- [x] 7.2 Add safe file semantic summaries, transactional first-seen work, idempotent memberships, exact aggregate counts, and release-scoped summaries.
- [x] 7.3 Extend runtime inventory, navigation, occurrence APIs, and release diff classification to expose file activity without host paths, contents, mount IDs, or inode IDs.
- [x] 7.4 Update OpenAPI schemas, examples, fixtures, query projections, and contract tests for each file operation and rename replacement state.

## 8. Telemetry and operational safety

- [x] 8.1 Extend heartbeat protocol and agent/server metrics with bounded file filtering, path, unsupported-object, attribution, correlation, aggregation, decode, rate-limit, and kernel-loss counters.
- [x] 8.2 Verify metrics and logs never use or emit file paths, workload values, file contents, or written bytes on rejection and overload paths.
- [x] 8.3 Add health/readiness diagnostics that identify the unsupported hook/helper/profile reason without claiming partial file coverage.

## 9. Verification and rollout documentation

- [x] 9.1 Add unit and integration coverage for path-boundary filtering, exclusion precedence, relative/symlink limitation behavior, tracked descriptor writes, aggregation timing, flush ordering, and rename scope transitions.
- [x] 9.2 Add bounded Kubernetes fixtures for selected and unselected workloads that create, repeatedly modify, truncate, delete, and rename included/excluded regular files.
- [ ] 9.3 Run verifier and syscall tracepoint acceptance on every claimed kernel profile and record any supported-platform adjustment before enabling the capability.
- [ ] 9.4 Add end-to-end tests from kernel observation through attribution, transport, durable raw storage, grouping, first-seen work, inventory, occurrences, and release diff.
- [x] 9.5 Document configuration, five-second visibility delay, syscall-path semantics and size contract, relative/symlink/mmap limitations, privacy boundary, counters, troubleshooting, staged enablement, and disable-only rollback.
- [ ] 9.6 Validate the complete OpenSpec change and run the relevant Rust, migration, protobuf, OpenAPI, and deployment manifest test suites.
