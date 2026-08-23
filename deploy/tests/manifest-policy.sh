#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d /tmp/okoscope-manifest-test.XXXXXX)
trap 'rm -rf "$work"' EXIT
commit=1111111111111111111111111111111111111111
web=ghcr.io/ihippik/okoscope-web:2222222222222222222222222222222222222222

cd "$root"
deploy/scripts/render-release.sh "$work/no-routing" "$commit" "$commit" "$web" disabled
upgrade="$work/no-routing/03-upgrade.yaml"
migrate=$(find "$work/no-routing" -name '02-migrate-*.yaml' -print -quit)
check=$(find "$work/no-routing" -name '02-notification-check-*.yaml' -print -quit)

! grep -q '^kind: Secret$' "$upgrade"
! grep -q '^kind: StatefulSet$' "$upgrade"
! grep -Eq 'image: .*:(latest|main|dev)$' "$upgrade" "$migrate"
[[ $(grep -c 'resources:' "$upgrade") -ge 3 ]]
[[ $(grep -c 'resources:' "$migrate") -eq 1 ]]
grep -q 'OKOSCOPE_MIGRATE: "false"' "$upgrade"
grep -q 'okoscope.io/required-migration: "13"' "$migrate"
grep -q 'OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED: "false"' "$upgrade" "$check"
grep -Eq 'okoscope.io/notification-config: "[0-9a-f]{12}"' "$upgrade"
grep -q 'notification_delivery_enabled=false' "$work/no-routing/PROVENANCE.txt"
! grep -Eq '(database-url=|credential=|encryption-key=|signing)' "$work/no-routing/PROVENANCE.txt"
[[ ! -e "$work/no-routing/04-routing.yaml" ]]

export OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED=true
export OKOSCOPE_NOTIFICATION_CONCURRENCY=4
export OKOSCOPE_NOTIFICATION_DRAIN_SECONDS=20
deploy/scripts/render-release.sh "$work/enabled" "$commit" "$commit" "$web" disabled
grep -q 'OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED: "true"' "$work/enabled/03-upgrade.yaml"
grep -q 'OKOSCOPE_NOTIFICATION_CONCURRENCY: "4"' "$work/enabled/03-upgrade.yaml"
grep -q 'notification_drain_seconds=20' "$work/enabled/PROVENANCE.txt"
disabled_check_name=$(awk '$1 == "name:" && $2 ~ /^okoscope-notification-check-/ {print $2; exit}' "$check")
enabled_check=$(find "$work/enabled" -name '02-notification-check-*.yaml' -print -quit)
enabled_check_name=$(awk '$1 == "name:" && $2 ~ /^okoscope-notification-check-/ {print $2; exit}' "$enabled_check")
[[ $disabled_check_name != "$enabled_check_name" ]]
disabled_fingerprint=$(awk '$1 == "okoscope.io/notification-config:" {gsub(/"/, "", $2); print $2; exit}' "$upgrade")
enabled_fingerprint=$(awk '$1 == "okoscope.io/notification-config:" {gsub(/"/, "", $2); print $2; exit}' "$work/enabled/03-upgrade.yaml")
[[ $disabled_fingerprint != "$enabled_fingerprint" ]]
export OKOSCOPE_NOTIFICATION_CONCURRENCY=0
if deploy/scripts/render-release.sh "$work/invalid" "$commit" "$commit" "$web" disabled >/dev/null 2>&1; then
  echo "invalid notification bounds unexpectedly rendered" >&2
  exit 1
fi
unset OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED OKOSCOPE_NOTIFICATION_CONCURRENCY OKOSCOPE_NOTIFICATION_DRAIN_SECONDS

export OKOSCOPE_DOMAIN=okoscope.example
export OKOSCOPE_CERTIFICATE_NAME=okoscope-example
export OKOSCOPE_CERT_ISSUER=letsencrypt-production
export OKOSCOPE_TLS_SECRET=okoscope-example-tls
export OKOSCOPE_HTTP_ENTRYPOINT=web
export OKOSCOPE_HTTPS_ENTRYPOINT=websecure
export OKOSCOPE_SERVER_SERVICE=okoscope-server
export OKOSCOPE_WEB_SERVICE=okoscope-web
deploy/scripts/render-release.sh "$work/routing" "$commit" "$commit" "$web" enabled
routing="$work/routing/04-routing.yaml"
[[ $(grep -c '^kind: IngressRoute$' "$routing") -eq 2 ]]
[[ $(grep -c '^kind: Middleware$' "$routing") -eq 1 ]]
[[ $(grep -c '^kind: Certificate$' "$routing") -eq 1 ]]
grep -Fq 'PathPrefix(`/api`)' "$routing"
grep -q 'name: okoscope-web' "$routing"
! grep -q '__[A-Z_]*__' "$routing"

echo "manifest policy tests passed"
