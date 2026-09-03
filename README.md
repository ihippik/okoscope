# Okoscope

Okoscope is a self-hosted, eBPF-powered runtime observability service for Linux and Kubernetes. It observes process execution and an explicit syscall allowlist for selected Deployments, attributes events to Kubernetes identities, sends them over a bidirectional gRPC session, stores them in PostgreSQL, and groups repeated behavior into Sentry-like runtime groups.

The repository is a Rust workspace containing the eBPF program, node agent, protocol, event model, and server. The web UI is developed separately.

```sh
make build
make test
make check
make build-ebpf # Linux with nightly Rust and bpf-linker
```

See [Outbound network observation](docs/outbound-network-observation.md) for the opt-in `network.connect` capability, privacy boundary, counters, and rollout order.

See [platform support](docs/platform-support.md) and [deployment and verification](docs/deployment.md).

See [runtime event retention](docs/runtime-events-retention.md) for Organization/Project policies, lightweight daily snapshots, and historical coverage.
