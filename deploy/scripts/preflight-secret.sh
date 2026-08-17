#!/usr/bin/env bash
set -euo pipefail

namespace=${1:-okoscope}
secret_name=${2:-okoscope-secrets}
required=(database-url postgres-password cluster-credential api-credential webhook-encryption-key)

fail() {
  echo "secret preflight failed: $1" >&2
  exit 1
}

kubectl get secret "$secret_name" -n "$namespace" >/dev/null || fail "required Secret is unavailable"
for key in "${required[@]}"; do
  encoded=$(kubectl get secret "$secret_name" -n "$namespace" -o "jsonpath={.data.${key}}")
  [[ -n $encoded ]] || fail "required key '$key' is missing or empty"
done

database_url=$(kubectl get secret "$secret_name" -n "$namespace" -o 'jsonpath={.data.database-url}' | base64 --decode)
[[ $database_url =~ ^postgres(ql)?://[^[:space:]]+$ ]] || fail "database-url has an invalid shape"

webhook_key=$(kubectl get secret "$secret_name" -n "$namespace" -o 'jsonpath={.data.webhook-encryption-key}' | base64 --decode)
[[ $webhook_key =~ ^[0-9a-fA-F]{64}$ ]] || fail "webhook-encryption-key must encode exactly 32 bytes as hex"
[[ $webhook_key != 0000000000000000000000000000000000000000000000000000000000000000 ]] || fail "webhook-encryption-key uses the development value"

unset database_url webhook_key encoded
echo "secret preflight passed for $namespace/$secret_name (values redacted)"

