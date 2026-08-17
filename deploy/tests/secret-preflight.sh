#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d /tmp/okoscope-secret-test.XXXXXX)
trap 'rm -rf "$work"' EXIT

cat >"$work/kubectl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
key=${*: -1}
if [[ $key != jsonpath=* ]]; then
  exit 0
fi
key=${key#jsonpath=\{.data.}
key=${key%\}}
case "$key" in
  database-url) value=${TEST_DATABASE_URL-} ;;
  postgres-password) value=${TEST_POSTGRES_PASSWORD-} ;;
  cluster-credential) value=${TEST_CLUSTER_CREDENTIAL-} ;;
  api-credential) value=${TEST_API_CREDENTIAL-} ;;
  webhook-encryption-key) value=${TEST_WEBHOOK_KEY-} ;;
  *) value= ;;
esac
if [[ -n $value ]]; then
  printf %s "$value" | base64
fi
exit 0
MOCK
chmod +x "$work/kubectl"
export PATH="$work:$PATH"
export TEST_DATABASE_URL='postgres://user:password@postgres:5432/okoscope'
export TEST_POSTGRES_PASSWORD='password'
export TEST_CLUSTER_CREDENTIAL='cluster-token'
export TEST_API_CREDENTIAL='api-token'
export TEST_WEBHOOK_KEY='0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'

deploy_log="$work/output"
cd "$root"
deploy/scripts/preflight-secret.sh okoscope okoscope-secrets >"$deploy_log" 2>&1
grep -q 'values redacted' "$deploy_log"
! grep -q "$TEST_API_CREDENTIAL" "$deploy_log"

export TEST_API_CREDENTIAL=
if deploy/scripts/preflight-secret.sh okoscope okoscope-secrets >"$deploy_log" 2>&1; then
  echo "missing key unexpectedly passed" >&2
  exit 1
fi
grep -q 'api-credential' "$deploy_log"

export TEST_API_CREDENTIAL='api-token'
export TEST_WEBHOOK_KEY='0000000000000000000000000000000000000000000000000000000000000000'
if deploy/scripts/preflight-secret.sh okoscope okoscope-secrets >"$deploy_log" 2>&1; then
  echo "development key unexpectedly passed" >&2
  exit 1
fi
! grep -q "$TEST_API_CREDENTIAL" "$deploy_log"

grep -q 'webhook-encryption-key: "0000000000000000000000000000000000000000000000000000000000000000"' \
  deploy/kubernetes/overlays/development/secret.yaml
! grep -R -q '^kind: Secret$' deploy/kubernetes/base deploy/kubernetes/overlays/production
echo "secret preflight tests passed"
