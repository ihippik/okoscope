## Context

The current agent attaches tracepoints and selected kprobes, emits fixed-size kernel records through bounded ring buffers, resolves Kubernetes attribution in userspace, and delivers typed events through a versioned protocol. The first file-observation iteration deliberately uses syscall pathname arguments so it can be evaluated before committing to filesystem-resolved paths. Entry/exit correlation proves syscall success, while a bounded `(TGID, fd, generation)` map connects writes to paths retained from successful opens.

The profile is explicitly named `file.activity.syscall-path/v1`: its path is process input, not a canonical inode path. It rejects relative paths instead of consulting `/proc` or reconstructing a dirfd and does not resolve symbolic links. Paths remain sensitive metadata, so configuration narrows capture before delivery and no event may contain contents, byte buffers, guessed paths, truncated paths, or excluded names.

## Goals / Non-Goals

**Goals:**

- Observe successful create, modify, delete, and rename activity for regular files in selected Kubernetes workloads.
- Deliver bounded normalized absolute pathname arguments supplied by the acting process and identify their reduced semantics in the capability name.
- Require inclusive path scope and support higher-priority exclusions with component-boundary matching.
- Collapse repeated modification activity into a code-defined five-second window.
- Preserve bounded resource use, explicit loss accounting, deterministic grouping, release comparison, and backward-compatible transport.

**Non-Goals:**

- Capturing contents, write buffers, byte counts, diffs, ownership, permissions, extended attributes, or unrestricted process arguments.
- Observing directories, symlink objects, sockets, devices, or every individual write syscall.
- Translating a process-visible path into a container image layer, persistent-volume identity, or host filesystem path.
- Resolving relative paths, `dirfd`, symlinks, hard links, bind-mount aliases, or canonical filesystem-object paths in the experimental profile.
- Guaranteeing coverage for memory-mapped dirty-page writeback in the first version; the advertised capability documents the supported mutation hook set.
- Reconstructing activity that occurred while the agent was stopped or its bounded buffers were exhausted.

## Decisions

### Use syscall entry/exit correlation for the experimental profile

File observation is a separate kernel record stream and typed event family. Entry probes copy only bounded pathname arguments and flags; exit probes emit only after a successful result. Successful `openat` returns populate bounded `(TGID, fd)` state with a generation and retained absolute path. Successful write calls use that state; successful close removes it. Unlink, truncate, and rename retain bounded entry context keyed by `pid_tgid` until their exit.

The loader validates every syscall tracepoint required by the configured operation set and attaches it atomically from the agent's readiness perspective. Partial attachment is cleaned up and never advertised. `file.create` is emitted only when a successful `openat` uses `O_CREAT|O_EXCL`; `O_CREAT` alone cannot prove whether the file already existed.

Alternatives considered:

- Filesystem-resolved BTF/LSM/VFS probes provide stronger identity but helper allowlists and success semantics require more cross-kernel research; this is deferred until the syscall-path result can be evaluated.
- Reading `/proc/<pid>/fd` or cwd in userspace is vulnerable to process exit, descriptor reuse, namespace races, and deleted names, so the experimental profile does not use it.
- LSM hooks provide useful filesystem objects but are authorization-time hooks and alone do not prove the operation ultimately succeeded.
- `fanotify` provides useful notifications but complicates per-container mount namespaces, node-wide watch management, and existing eBPF/cgroup attribution.

### Retain only bounded absolute syscall pathname arguments

Probes copy pathname arguments with a fixed user-string bound. A path enters correlation state only if it is complete, normalized, absolute, NUL-free, and within the bound. Relative, unreadable, unterminated, non-normalized, and oversized arguments have distinct monotonic counters. Records identify path semantics as `syscall_argument_v1` and never describe the value as resolved or canonical.

For structural operations, old and new pathname arguments are captured before the syscall and retained only until successful return. For writes, the path and generation retained from a successful tracked open are associated with the descriptor. Descriptor replacement updates the generation so stale write correlation cannot inherit an older path.

Truncating or resolving a relative path from later `/proc` state is rejected because either creates false identity and can bypass filters. The trade-off is deliberate under-coverage.

### Filter complete normalized paths with exclusion precedence

Configuration adds a strict default-disabled `observation.files` object. When enabled it requires a non-empty operations set and at least one `includePaths` entry; `excludePaths` defaults to empty. Paths must be absolute and lexically normalized at configuration load. Equality or descendant matching occurs only on component boundaries. Exclusion is evaluated after inclusion and wins.

The syscall-path v1 probe rejects relative, unreadable, and oversized arguments in kernel space. Variable-length configured prefixes are not placed in kernel maps because bounded prefix iteration would materially increase verifier complexity on the 5.15 baseline; authoritative include/exclude matching occurs immediately after fixed-record decoding and before attribution or delivery. No path is used as a metric label or written to diagnostic logs.

For rename, each side is classified independently:

| Old path | New path | Output |
|---|---|---|
| observable | observable | `file.rename(oldPath, newPath, replaced)` |
| not observable | observable | `file.create(newPath)` |
| observable | not observable | `file.delete(oldPath)` |
| not observable | not observable | none |

An excluded side is treated as not observable and is never included in the transformed event.

### Aggregate modify occurrences in userspace for a fixed five-second window

The code defines `FILE_MODIFY_AGGREGATION_WINDOW` as five seconds; it is intentionally not configuration. Aggregation is keyed by trusted workload UID, container identity, process TGID, descriptor, and descriptor generation. Each entry retains the permitted absolute syscall path and relevant process identity observed during the window.

A bounded expiry structure emits one `file.modify` when the window closes. Rename or delete of the inode flushes the pending modify first, preserving causal order. Container termination and orderly agent shutdown flush eligible entries; capacity exhaustion follows a documented eviction/drop policy and increments a dedicated counter. Global rate, queue, batch, replay, and delivery limits apply after aggregation.

Kernel-side time aggregation was rejected because it would duplicate attribution/filter policy, complicate ordered structural events, and consume less observable kernel state.

### Add operation-specific typed payloads and safe identities

The shared model and protobuf add typed payloads for create, modify, delete, and rename. Create/modify/delete carry one normalized path; rename carries ordered old/new paths and `replaced`. All carry existing process and Kubernetes attribution through `RuntimeEvent`; kernel-only mount/inode identifiers support local correlation but do not define cross-release group identity and need not be exposed by public APIs.

Server validation repeats path and operation invariants. Group fingerprints use trusted scope, event kind, normalized process command, and public path identity; rename additionally uses both ordered paths and replacement identity. Pod, container, PID, mount ID, inode, timestamps, and release remain occurrence fields so equivalent behavior remains comparable across rollouts.

### Extend capability and loss telemetry additively

An enabled and fully attached agent advertises `file.activity.syscall-path/v1`. Heartbeats add counters for filtering, relative/non-normalized path rejection, user-memory read failure, path oversize, missing descriptor mapping, attribution, entry/exit correlation, aggregation capacity, decode, rate limiting, and kernel/ring loss. Metrics use bounded reason labels only.

## Risks / Trade-offs

- [Relative paths and filesystem-resolved identity are absent] → Reject and count relative paths, name the weaker profile in capabilities/APIs, and evaluate a resolved-path v2 separately.
- [An absolute syscall path may traverse symlinks or name another object type] → Preserve it as reported evidence, never label it canonical, and correlate modifications only through successfully tracked descriptors.
- [Memory-mapped modifications can bypass initial write hooks] → State this as a v1 limitation, count/advertise the exact capability profile, and evaluate a later dirty-page or fsnotify-backed extension separately.
- [Rename races and overwritten destinations complicate identity] → Capture both paths and replacement state before mutation, correlate through success, and drop rather than guess when correlation is missing.
- [Path metadata can be sensitive] → Require includes, prioritize exclusions, never log paths on rejection, bound all fields, and omit paths from telemetry labels.
- [High file churn can exhaust maps or queues] → Aggregate modifications, filter early, bound every stage, expose reason counters, and exercise overload acceptance tests.
- [Five-second aggregation delays modify visibility] → Keep create/delete/rename immediate, flush modify before structural events, and document the fixed latency/volume trade-off.

## Migration Plan

1. Add backward-compatible event, protocol, database, grouping, inventory, API, and heartbeat fields while file observation remains disabled by default.
2. Add syscall tracepoint profiles and startup capability checks, then validate tracepoint layouts and verifier behavior on every claimed kernel line.
3. Deploy server changes and migrations before file-capable agents; older agents continue sending existing events.
4. Roll out agents with file observation disabled, verify readiness and baseline counters, then enable it for one bounded selected fixture with narrow includes and exclusions.
5. Confirm selected/unselected workload isolation, path namespace behavior, aggregation ordering, loss counters, raw storage, grouping, inventory, and release diff before wider rollout.
6. Roll back by disabling `observation.files`; additive stored events and schema remain readable and no destructive database rollback is required.

## Open Questions

- What observed coverage and noise justify returning to a filesystem-resolved `file.activity/v2` profile?
- Should orderly shutdown wait up to a bounded deadline to flush pending modifications, or deliver them immediately with the last observation timestamp?
- What fixed path byte limit balances verifier/ring-buffer cost with real container paths: 512 or 1024 bytes?
