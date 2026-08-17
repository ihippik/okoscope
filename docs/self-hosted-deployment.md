# Self-hosted Kubernetes deployment

The hardened deployment workflow separates one-time stateful installation from repeatable application upgrades. Production release bundles never create or update `okoscope-secrets`, and the upgrade artifact never contains PostgreSQL resources.

## Requirements and release bundle

- Kubernetes with `kubectl` and Kustomize support.
- An existing `okoscope` namespace for external PostgreSQL, or the bundled install artifact for a new installation.
- An externally managed `okoscope-secrets` Secret.
- Immutable server, agent, and Web image references.
- Optional Traefik `traefik.io/v1alpha1` and cert-manager `cert-manager.io/v1` CRDs when public routing is enabled.

CI publishes an `okoscope-kubernetes-<commit>` bundle containing:

1. `01-install-bundled-postgres.yaml` — one-time namespace, Service, StatefulSet, and PVC template.
2. `02-migrate-<commit>.yaml` — release-specific migration gate.
3. `03-upgrade.yaml` — stateless server, Web, agent, Services, RBAC, configuration, and disruption controls.
4. `04-routing.yaml` — optional public route and certificate resources.
5. `PROVENANCE.txt` — source/image mapping and required migration.

The bundled PostgreSQL profile requests 100m CPU/256 MiB and limits 1 CPU/1 GiB. Server defaults are 100m/128 MiB requests and 1 CPU/512 MiB limits; agent defaults are 100m/96 MiB and 1 CPU/512 MiB. Tune these in a site overlay after measuring usage.

The agent is the only host-aware workload. `hostPID` is required to map kernel PIDs to containers; read-only `/proc` and cgroup v2 mounts provide attribution; tracefs is writable for probe attachment. The container drops every capability except `BPF`, `PERFMON`, and `SYS_RESOURCE`, and RBAC is read-only for Pods, ReplicaSets, and Deployments. It has no host network, host root mount, Secret read API, workload mutation, or broad `privileged` mode. Older kernels that lack capability separation are unsupported by this hardened profile rather than silently receiving `SYS_ADMIN`.

## Provision the Secret

Use [the placeholder schema](../deploy/examples/okoscope-secret.example.yaml) only as a key inventory. Create real values without writing a populated YAML file:

```bash
kubectx aliens
read -rs OKOSCOPE_DATABASE_URL
printf '\n'
read -rs OKOSCOPE_POSTGRES_PASSWORD
printf '\n'
kubectl create secret generic okoscope-secrets -n okoscope \
  --from-literal=database-url="$OKOSCOPE_DATABASE_URL" \
  --from-literal=postgres-password="$OKOSCOPE_POSTGRES_PASSWORD" \
  --from-literal=cluster-credential="$(openssl rand -hex 32)" \
  --from-literal=api-credential="$(openssl rand -hex 32)" \
  --from-literal=webhook-encryption-key="$(openssl rand -hex 32)"
unset OKOSCOPE_DATABASE_URL OKOSCOPE_POSTGRES_PASSWORD
```

For an existing installation, do not run `create` again. Rotate one key with a server-side merge so unspecified keys survive:

```bash
kubectx aliens
kubectl patch secret okoscope-secrets -n okoscope --type merge \
  -p "{\"stringData\":{\"api-credential\":\"$(openssl rand -hex 32)\"}}"
```

Rotation invalidates clients using the old value. Coordinate agent and UI credential changes before restarting workloads. Never put values in shell history, tickets, CI artifacts, or repository files. The preflight reports key names and validation reasons only.

## New bundled installation

Review the StorageClass and requested size in the install artifact before first use; those StatefulSet fields are intentionally not part of upgrades.

```bash
kubectx aliens
kubectl apply -f 01-install-bundled-postgres.yaml
# Provision okoscope-secrets, then:
deploy/scripts/preflight-secret.sh okoscope okoscope-secrets
OKOSCOPE_INSTALL_BUNDLED_POSTGRES=false deploy/scripts/deploy-release.sh ./release
```

For external PostgreSQL, create the namespace and Secret with the external `database-url`, skip `01-install-bundled-postgres.yaml`, and use the same migration and upgrade artifacts.

## Adopt an existing MVP installation

Adoption is non-destructive. Before the first hardened release, record and compare the live identities:

```bash
kubectx aliens
kubectl get secret okoscope-secrets -n okoscope -o jsonpath='{.metadata.uid}{"\n"}'
kubectl get statefulset postgres -n okoscope -o jsonpath='{.metadata.uid}{"\n"}'
kubectl get pvc -n okoscope -l app=postgres
kubectl get service okoscope-server postgres -n okoscope
kubectl get ingressroute,middleware -n okoscope
kubectl get certificate -n okoscope
```

Do not apply the install artifact to the adopted environment. Keep the existing Secret, StatefulSet, PVC, Service names, Traefik routes, and Certificate until rendered resources have been diffed. The upgrade artifact adopts stateless resources by stable names and does not take ownership of Secret or PostgreSQL. If existing labels differ, update selectors only after confirming they continue to match live Pods.

The 2026-08-17 `aliens` adoption dry-run found and corrected two compatibility issues before rollout: Kustomize labels were initially entering immutable workload selectors, and the existing Web Service exposes port 80 rather than 8080. After preserving the live `app` selectors and Service ports, server-side dry-run accepted the server, Web, agent, PDB, Certificate, Middleware, and both IngressRoutes without mutating the cluster. The live Secret still has the intentionally retained development webhook key, so production preflight and actual migration/rollout remain gated until that key is provisioned out of band and images containing `server migrate` are published.

## Ordered upgrade and failure gate

The canonical sequence is render, validate, secret preflight, migration, rollout, then smoke verification. `deploy-release.sh` stops immediately when preflight fails or the migration Job does not complete; it never applies `03-upgrade.yaml` after such a failure.

```bash
deploy/scripts/render-release.sh ./release \
  "$SERVER_COMMIT" "$AGENT_COMMIT" "$WEB_IMAGE" disabled
deploy/tests/manifest-policy.sh
deploy/tests/secret-preflight.sh
deploy/scripts/deploy-release.sh ./release
```

The production server has `OKOSCOPE_MIGRATE=false`. Only the release-specific Job runs `server migrate`. Reapplying an already completed release is safe; migration history and credentials are preserved.

For public routing, set `OKOSCOPE_DOMAIN`, `OKOSCOPE_CERT_ISSUER`, `OKOSCOPE_TLS_SECRET`, `OKOSCOPE_HTTP_ENTRYPOINT`, `OKOSCOPE_HTTPS_ENTRYPOINT`, `OKOSCOPE_SERVER_SERVICE`, and `OKOSCOPE_WEB_SERVICE`, then render with the final argument `enabled`. Invalid or missing values fail before rendering.

## Verification and rollback

After rollout, verify `/readyz`, `/api/v1/build-info`, server migration logs, connected agent sessions, Certificate readiness, the HTTPS redirect, `/api` routing, Web fallback, and one bounded runtime-event smoke. Record the image IDs and database migration from `PROVENANCE.txt`.

If application readiness fails after a successful additive migration, render `03-upgrade.yaml` with the previous compatible image commits and apply it. Do not delete or roll back migration rows, Jobs, the Secret, StatefulSet, or PVC. If the previous server is not forward-compatible with the recorded migration, stop rather than forcing rollback.

## PostgreSQL durability

Bundled PostgreSQL is single-replica and is not highly available. A PVC protects against Pod replacement, not operator deletion, storage failure, or corruption. Schedule logical backups with `pg_dump`, encrypt and store them outside the cluster, and regularly test restore into a separate database. Snapshot support is storage-provider-specific. Internet-facing or production installations should use a managed/external PostgreSQL service with automated backups, point-in-time recovery, monitoring, and a documented recovery objective.

## Legacy artifact transition

`deploy/kubernetes/mvp.yaml` is deprecated. It embeds development credentials, runs startup migrations, and combines PostgreSQL with stateless upgrades. It remains for one release only to help compare existing resources; do not use it for new installs or upgrades. Recovery during the transition uses the last known immutable application images through `03-upgrade.yaml`, never the monolithic manifest.
