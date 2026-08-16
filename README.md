# Okoscope

Okoscope is a self-hosted, eBPF-powered runtime observability service for Linux and Kubernetes. The MVP observes process execution and an explicit syscall allowlist for selected Deployments, attributes events to Kubernetes identities, sends them over a bidirectional gRPC session, and stores them in PostgreSQL.

The repository is a Rust workspace containing the eBPF program, node agent, protocol, event model, and server. The web UI is developed separately.

```sh
make build
make test
make check
make build-ebpf # Linux with nightly Rust and bpf-linker
```

See [platform support](docs/platform-support.md) and [deployment and verification](docs/deployment.md).
