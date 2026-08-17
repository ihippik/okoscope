#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <output-dir> <server-tag> <agent-tag> <web-image> [routing]" >&2
  exit 2
}

[[ $# -ge 4 && $# -le 5 ]] || usage
output_dir=$1
server_tag=$2
agent_tag=$3
web_image=$4
routing=${5:-disabled}

[[ $server_tag =~ ^[0-9a-f]{40}$ ]] || { echo "server tag must be a 40-character commit SHA" >&2; exit 2; }
[[ $agent_tag =~ ^[0-9a-f]{40}$ ]] || { echo "agent tag must be a 40-character commit SHA" >&2; exit 2; }
[[ $web_image =~ ^[^[:space:]]+(@sha256:[0-9a-f]{64}|:[0-9a-f]{40})$ ]] || {
  echo "web image must use a commit tag or sha256 digest" >&2
  exit 2
}
[[ $routing == enabled || $routing == disabled ]] || usage

mkdir -p "$output_dir"
kubectl kustomize deploy/kubernetes/install/bundled-postgres >"$output_dir/01-install-bundled-postgres.yaml"
kubectl kustomize deploy/kubernetes/migrate \
  | sed -e "s/0000000000000000000000000000000000000000/$server_tag/g" \
        -e "s/okoscope-migrate-000000000000/okoscope-migrate-${server_tag:0:12}/g" \
  >"$output_dir/02-migrate-${server_tag:0:12}.yaml"
kubectl kustomize deploy/kubernetes/overlays/production \
  | sed -e "s#ghcr.io/ihippik/okoscope-server:0000000000000000000000000000000000000000#ghcr.io/ihippik/okoscope-server:$server_tag#g" \
        -e "s#ghcr.io/ihippik/okoscope-agent:0000000000000000000000000000000000000000#ghcr.io/ihippik/okoscope-agent:$agent_tag#g" \
        -e "s#ghcr.io/ihippik/okoscope-web:0000000000000000000000000000000000000000#$web_image#g" \
  >"$output_dir/03-upgrade.yaml"

if [[ $routing == enabled ]]; then
  : "${OKOSCOPE_DOMAIN:?OKOSCOPE_DOMAIN is required}"
  : "${OKOSCOPE_CERTIFICATE_NAME:?OKOSCOPE_CERTIFICATE_NAME is required}"
  : "${OKOSCOPE_CERT_ISSUER:?OKOSCOPE_CERT_ISSUER is required}"
  : "${OKOSCOPE_TLS_SECRET:?OKOSCOPE_TLS_SECRET is required}"
  : "${OKOSCOPE_HTTP_ENTRYPOINT:?OKOSCOPE_HTTP_ENTRYPOINT is required}"
  : "${OKOSCOPE_HTTPS_ENTRYPOINT:?OKOSCOPE_HTTPS_ENTRYPOINT is required}"
  : "${OKOSCOPE_SERVER_SERVICE:?OKOSCOPE_SERVER_SERVICE is required}"
  : "${OKOSCOPE_WEB_SERVICE:?OKOSCOPE_WEB_SERVICE is required}"
  [[ $OKOSCOPE_DOMAIN =~ ^[A-Za-z0-9.-]+$ ]] || { echo "invalid domain" >&2; exit 2; }
  for value in "$OKOSCOPE_CERTIFICATE_NAME" "$OKOSCOPE_CERT_ISSUER" "$OKOSCOPE_TLS_SECRET" "$OKOSCOPE_HTTP_ENTRYPOINT" "$OKOSCOPE_HTTPS_ENTRYPOINT" "$OKOSCOPE_SERVER_SERVICE" "$OKOSCOPE_WEB_SERVICE"; do
    [[ $value =~ ^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$ ]] || { echo "invalid Kubernetes routing input" >&2; exit 2; }
  done
  kubectl kustomize deploy/kubernetes/routing \
    | sed -e "s/__DOMAIN__/$OKOSCOPE_DOMAIN/g" \
          -e "s/__CERTIFICATE_NAME__/$OKOSCOPE_CERTIFICATE_NAME/g" \
          -e "s/__CERT_ISSUER__/$OKOSCOPE_CERT_ISSUER/g" \
          -e "s/__TLS_SECRET__/$OKOSCOPE_TLS_SECRET/g" \
          -e "s/__HTTP_ENTRYPOINT__/$OKOSCOPE_HTTP_ENTRYPOINT/g" \
          -e "s/__HTTPS_ENTRYPOINT__/$OKOSCOPE_HTTPS_ENTRYPOINT/g" \
          -e "s/__SERVER_SERVICE__/$OKOSCOPE_SERVER_SERVICE/g" \
          -e "s/__WEB_SERVICE__/$OKOSCOPE_WEB_SERVICE/g" \
    >"$output_dir/04-routing.yaml"
fi

cat >"$output_dir/PROVENANCE.txt" <<EOF
server_image=ghcr.io/ihippik/okoscope-server:$server_tag
agent_image=ghcr.io/ihippik/okoscope-agent:$agent_tag
web_image=$web_image
required_migration=6
routing=$routing
EOF
