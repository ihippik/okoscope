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

! grep -q '^kind: Secret$' "$upgrade"
! grep -q '^kind: StatefulSet$' "$upgrade"
! grep -Eq 'image: .*:(latest|main|dev)$' "$upgrade" "$migrate"
[[ $(grep -c 'resources:' "$upgrade") -ge 3 ]]
[[ $(grep -c 'resources:' "$migrate") -eq 1 ]]
grep -q 'OKOSCOPE_MIGRATE: "false"' "$upgrade"
grep -q 'okoscope.io/required-migration: "6"' "$migrate"
[[ ! -e "$work/no-routing/04-routing.yaml" ]]

export OKOSCOPE_DOMAIN=okoscope.example
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

