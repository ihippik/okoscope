#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

helm dependency build "$root/deploy/helm/okoscope" --skip-refresh
helm lint "$root/deploy/helm/okoscope"
helm lint "$root/deploy/helm/okoscope-agent" -f "$root/deploy/helm/fixtures/agent-hosted.yaml"

helm template okoscope "$root/deploy/helm/okoscope" > "$work/self-hosted.yaml"
helm template okoscope "$root/deploy/helm/okoscope" -f "$root/deploy/helm/fixtures/self-hosted-ingress.yaml" > "$work/self-hosted-ingress.yaml"
helm template okoscope "$root/deploy/helm/okoscope" -f "$root/deploy/helm/fixtures/self-hosted-agent.yaml" > "$work/self-hosted-agent.yaml"
helm template agent "$root/deploy/helm/okoscope-agent" -f "$root/deploy/helm/fixtures/agent-hosted.yaml" > "$work/agent.yaml"
helm template agent "$root/deploy/helm/okoscope-agent" -f "$root/deploy/helm/fixtures/agent-labels.yaml" > "$work/agent-labels.yaml"

for manifest in "$work"/*.yaml; do
  if grep -Eq 'oko_app_v1_|postgresql://[^[:space:]]+:[^[:space:]]+@|image: .+:(latest|main|master)([[:space:]]|$)|__[A-Z0-9_]+__' "$manifest"; then
    echo "unsafe or unresolved value in $manifest" >&2
    exit 1
  fi
done

if grep -Eq '^kind: (StatefulSet|PersistentVolumeClaim)$|name: postgres' "$work/self-hosted.yaml"; then
  echo "the Okoscope chart must not provision PostgreSQL" >&2
  exit 1
fi
grep -q 'helm.sh/hook: pre-install,pre-upgrade' "$work/self-hosted.yaml"
grep -q 'name: production-database' "$work/self-hosted-ingress.yaml"
grep -q 'nginx.ingress.kubernetes.io/backend-protocol: GRPC' "$work/self-hosted-ingress.yaml"
helm template okoscope "$root/deploy/helm/okoscope" -f "$root/deploy/helm/fixtures/self-hosted-ingress.yaml" \
  --set ingress.grpc.className=traefik > "$work/self-hosted-traefik.yaml"
grep -q 'traefik.ingress.kubernetes.io/service.serversscheme: h2c' "$work/self-hosted-traefik.yaml"
grep -q '^kind: DaemonSet$' "$work/self-hosted-agent.yaml"
grep -q 'name: okoscope-application-credentials' "$work/agent.yaml"
grep -q 'name: payments-observability' "$work/agent-labels.yaml"
grep -q 'name: worker-observability' "$work/agent-labels.yaml"

if helm template rejected "$root/deploy/helm/okoscope" --set database.url=postgresql://unsafe >/dev/null 2>&1; then
  echo 'database.url must be rejected by the values schema' >&2
  exit 1
fi
if helm template rejected "$root/deploy/helm/okoscope" -f "$root/deploy/helm/fixtures/self-hosted-ingress.yaml" --set server.registrationEnabled=true >/dev/null 2>&1; then
  echo 'public Web ingress with open registration must be rejected' >&2
  exit 1
fi
grep -q 'OKOSCOPE_REGISTRATION_ENABLED: "false"' "$work/self-hosted.yaml"
grep -q 'name: OKOSCOPE_SETUP_TOKEN' "$work/self-hosted.yaml"
grep -A1 'name: OKOSCOPE_API_BASE_URL' "$work/self-hosted.yaml" | grep -q 'value: /'
grep -A1 'name: OKOSCOPE_API_UPSTREAM' "$work/self-hosted.yaml" | grep -q 'value: http://okoscope-server:8080'
grep -q 'kind: Secret' "$work/self-hosted.yaml"
helm template custom-ca "$root/deploy/helm/okoscope" \
  --set agentInstallation.publicGrpcEndpoint=grpc.example.com:443 \
  --set agentInstallation.tlsMode=custom_ca \
  --set agentInstallation.caSecret.name=okoscope-private-ca \
  --set agentInstallation.caSecret.key=root.crt > "$work/self-hosted-custom-ca.yaml"
grep -q 'OKOSCOPE_AGENT_CA_SECRET_NAME: "okoscope-private-ca"' "$work/self-hosted-custom-ca.yaml"
grep -q 'OKOSCOPE_AGENT_CA_SECRET_KEY: "root.crt"' "$work/self-hosted-custom-ca.yaml"
if helm template rejected "$root/deploy/helm/okoscope" --set agentInstallation.tlsMode=custom_ca >/dev/null 2>&1; then
  echo 'custom_ca mode without a CA Secret name must be rejected' >&2
  exit 1
fi
if helm template rejected "$root/deploy/helm/okoscope" --set agentInstallation.caSecret.name=unexpected-ca >/dev/null 2>&1; then
  echo 'system roots mode with a CA Secret name must be rejected' >&2
  exit 1
fi
if helm template rejected "$root/deploy/helm/okoscope" --set setupAuthorization.token=unsafe >/dev/null 2>&1; then
  echo 'plaintext setup authorization must be rejected by the values schema' >&2
  exit 1
fi
if helm template rejected "$root/deploy/helm/okoscope-agent" -f "$root/deploy/helm/fixtures/agent-hosted.yaml" --set applicationCredential=oko_app_v1_unsafe >/dev/null 2>&1; then
  echo 'plaintext Application credentials must be rejected by the values schema' >&2
  exit 1
fi
if helm template rejected "$root/deploy/helm/okoscope-agent" -f "$root/deploy/helm/fixtures/agent-hosted.yaml" --set image.tag=latest >/dev/null 2>&1; then
  echo 'mutable production image tags must be rejected' >&2
  exit 1
fi

if command -v kubeconform >/dev/null; then
  kubeconform -strict -summary -ignore-missing-schemas "$work"/*.yaml
fi

true
