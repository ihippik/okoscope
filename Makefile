.PHONY: build build-ebpf check test proto-check deployment-test

build:
	cargo build --workspace --exclude agent-ebpf

build-ebpf:
	cargo +nightly build -p agent-ebpf --target bpfel-unknown-none -Z build-std=core

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --exclude agent-ebpf --all-targets -- -D warnings

test:
	cargo test --workspace --exclude agent-ebpf

proto-check:
	cargo check -p protocol

deployment-test:
	deploy/tests/manifest-policy.sh
	deploy/tests/secret-preflight.sh
	deploy/tests/deployment-workflow.sh
