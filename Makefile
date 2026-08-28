KUBE_CONTEXT ?= aliens
KUBE_NAMESPACE ?= okoscope
KUSTOMIZE_DIR ?= deploy/kubernetes/common

.PHONY: build build-ebpf check test proto-check deployment-test deploy-render deploy-diff migrate deploy deploy-status

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

migrate:
	@set -eu; \
	server_tag=$$(sed -n '/name: ghcr.io\/ihippik\/okoscope-server/{n;s/.*newTag: *"\([0-9a-f]*\)".*/\1/p;}' $(KUSTOMIZE_DIR)/kustomization.yaml); \
	required_migration=$$(sed -nE 's/^pub const REQUIRED_MIGRATION: i64 = ([0-9]+);$$/\1/p' crates/server/src/database.rs); \
	test $${#server_tag} -eq 40 || { echo "cannot read immutable server tag" >&2; exit 1; }; \
	test -n "$$required_migration" || { echo "cannot read REQUIRED_MIGRATION" >&2; exit 1; }; \
	short_tag=$$(printf '%s' "$$server_tag" | cut -c1-12); \
	work=$$(mktemp -d /tmp/okoscope-migrate.XXXXXX); \
	trap 'rm -rf "$$work"' EXIT; \
	kubectl kustomize deploy/kubernetes/server/migration \
		| sed -e "s/0000000000000000000000000000000000000000/$$server_tag/g" \
			-e "s/okoscope-migrate-000000000000/okoscope-migrate-$$short_tag/g" \
			-e "s/__REQUIRED_MIGRATION__/\"$$required_migration\"/g" \
		> "$$work/job.yaml"; \
	kubectx $(KUBE_CONTEXT); \
	kubectl apply -f deploy/kubernetes/common/namespace.yaml; \
	deploy/scripts/preflight-secret.sh $(KUBE_NAMESPACE); \
	kubectl apply -f "$$work/job.yaml"; \
	status=0; \
	kubectl wait --for=condition=complete --timeout=5m "job/okoscope-migrate-$$short_tag" -n $(KUBE_NAMESPACE) || status=$$?; \
	deploy/scripts/prune-job-history.sh $(KUBE_NAMESPACE) okoscope-migrate okoscope-notification-check; \
	exit $$status

deploy: deployment-test
	kubectx $(KUBE_CONTEXT)
	kubectl apply -f deploy/kubernetes/common/namespace.yaml
	deploy/scripts/preflight-secret.sh $(KUBE_NAMESPACE)
	$(MAKE) migrate
	kubectl apply -k $(KUSTOMIZE_DIR)
	kubectl rollout status deployment/okoscope-server -n $(KUBE_NAMESPACE) --timeout=5m
	kubectl rollout status daemonset/okoscope-agent -n $(KUBE_NAMESPACE) --timeout=5m
	kubectl rollout status deployment/okoscope-web -n $(KUBE_NAMESPACE) --timeout=5m

deploy-status:
	kubectx $(KUBE_CONTEXT)
	kubectl get deployment,daemonset,pod -n $(KUBE_NAMESPACE)
