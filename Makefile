KUBE_CONTEXT ?= aliens
KUBE_NAMESPACE ?= okoscope
KUSTOMIZE_DIR ?= deploy/kubernetes/common

.PHONY: build build-ebpf check test proto-check deployment-test deploy-render deploy-diff deploy deploy-status

build:
	cargo build --workspace --exclude agent-ebpf

build-ebpf:
	cargo +nightly build -p agent-ebpf --target bpfel-unknown-none -Z build-std=core
	$(MAKE) -C crates/agent-ebpf-core

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

deploy-render:
	kubectl kustomize $(KUSTOMIZE_DIR)

deploy-diff: deployment-test
	kubectx $(KUBE_CONTEXT)
	deploy/scripts/preflight-secret.sh $(KUBE_NAMESPACE)
	kubectl diff -k $(KUSTOMIZE_DIR)

deploy: deployment-test
	kubectx $(KUBE_CONTEXT)
	kubectl apply -f deploy/kubernetes/common/namespace.yaml
	deploy/scripts/preflight-secret.sh $(KUBE_NAMESPACE)
	kubectl apply -k $(KUSTOMIZE_DIR)
	kubectl rollout status deployment/okoscope-server -n $(KUBE_NAMESPACE) --timeout=5m
	kubectl rollout status daemonset/okoscope-agent -n $(KUBE_NAMESPACE) --timeout=5m
	kubectl rollout status deployment/okoscope-web -n $(KUBE_NAMESPACE) --timeout=5m

deploy-status:
	kubectx $(KUBE_CONTEXT)
	kubectl get deployment,daemonset,pod -n $(KUBE_NAMESPACE)
