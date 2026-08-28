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
3. `02-notification-check-<commit>.yaml` — secret-redacted notification activation and schema gate.
4. `03-upgrade.yaml` — stateless server, Web, agent, Services, RBAC, configuration, and disruption controls.
5. `04-routing.yaml` — optional public route and certificate resources.
6. `PROVENANCE.txt` — source/image mapping, required migration, activation state, and non-secret worker bounds.

Set the GitHub Actions repository variable `OKOSCOPE_WEB_IMAGE` to the current immutable Web image before publishing a bundle. Bundle rendering derives `required_migration` from the backend's `REQUIRED_MIGRATION` constant and uses that value in both schema-gate Jobs and `PROVENANCE.txt`; rendering fails instead of falling back to a stale Web image or migration version.

The bundled PostgreSQL profile requests 100m CPU/256 MiB and limits 1 CPU/1 GiB. Server defaults are 100m/128 MiB requests and 1 CPU/512 MiB limits; agent defaults are 100m/96 MiB and 1 CPU/512 MiB. Tune these in a site overlay after measuring usage.

The agent is the only host-aware workload. `hostPID` is required to map kernel PIDs to containers; read-only `/proc` and cgroup v2 mounts provide attribution; tracefs is writable for probe attachment. The container drops every capability except `BPF`, `PERFMON`, `SYS_RESOURCE`, and `SYS_ADMIN`; the latter is required by the reference cluster kernel for tracepoint `perf_event_open`. RBAC is read-only for Pods, ReplicaSets, Deployments, and the single `kube-system` Namespace used to discover its stable UID. It has no host network, host root mount, Secret read API, workload mutation, or broad `privileged` mode.

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
  --from-literal=admin-credential="$(openssl rand -hex 32)" \
  --from-literal=webhook-encryption-key="$(openssl rand -hex 32)"
unset OKOSCOPE_DATABASE_URL OKOSCOPE_POSTGRES_PASSWORD
```

For an existing installation, do not run `create` again. Human access uses individually revocable user sessions, so there is no shared tenant API credential to rotate. Never put bootstrap passwords, session values, Application tokens, or other secrets in shell history, tickets, CI artifacts, or repository files. The preflight reports key names and validation reasons only.

Before removing a legacy tenant API credential, migrate the database, create or resolve the target Organization, and establish its first owner through the one-shot command. Supply the password through a protected environment/secret injection rather than a command-line argument:

```bash
read -rs OKOSCOPE_BOOTSTRAP_OWNER_PASSWORD
export OKOSCOPE_BOOTSTRAP_OWNER_PASSWORD
export OKOSCOPE_BOOTSTRAP_OWNER_EMAIL='owner@example.com'
server --database-url "$OKOSCOPE_DATABASE_URL" bootstrap-owner \
  --organization-id '<organization-uuid>'
unset OKOSCOPE_BOOTSTRAP_OWNER_PASSWORD OKOSCOPE_BOOTSTRAP_OWNER_EMAIL
```

The command is idempotent: if the Organization already has an owner, it does not replace credentials or create another membership. Back up PostgreSQL before migration `0016`; rollback after the legacy table drop requires restoring that backup with the previous server release.

## Provision tenants and Application ingestion

The system admin credential creates Organizations, Projects, and Applications. Application creation returns a versioned `oko_app_v1_...` token exactly once; the database stores only its digest. Save that response directly into a secret-management workflow and never place the token in a ConfigMap, committed manifest, ticket, or log.

Create `okoscope-application-credentials` from the one-time response, with one key per Application, and project those keys as read-only files into the agent DaemonSet. Each workload selector references `applicationCredentialFile`; one agent process opens an independently bounded stream per distinct token. The agent reads the UID of `kube-system`, and the server automatically creates or reuses that Cluster inside the token-derived Organization.

Rotation is overlap-first: issue an additional credential, update the Kubernetes Secret and roll the DaemonSet, verify its `last_used_at`, then revoke the old credential through the admin API. Revocation stops the affected stream at its next batch without stopping other Applications. Adding or rotating credentials requires a DaemonSet rollout in this release; hot reload is not supported.

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

The 2026-08-17 `aliens` adoption dry-run found and corrected two compatibility issues before rollout: Kustomize labels were initially entering immutable workload selectors, and the existing Web Service exposes port 80 rather than 8080. The first live rollout then found two runtime-only requirements: the Web entrypoint creates `/tmp/okoscope-web`, and the reference kernel requires `SYS_ADMIN` for tracepoint `perf_event_open`. Web and agent were rolled back while remaining available; the manifests now provide a writable `/tmp` `emptyDir` and the explicit capability. Server migration and rollout succeeded without replacing Secret, PostgreSQL, or PVC identities.

## Ordered upgrade and failure gate

The canonical sequence is render, validate, secret preflight, migration, notification configuration check, rollout, then smoke verification. `deploy-release.sh` stops immediately when any gate fails; it never applies `03-upgrade.yaml` after such a failure.

```bash
deploy/scripts/render-release.sh ./release \
  "$SERVER_COMMIT" "$AGENT_COMMIT" "$WEB_IMAGE" disabled
deploy/tests/manifest-policy.sh
deploy/tests/secret-preflight.sh
deploy/scripts/deploy-release.sh ./release
```

The production server has `OKOSCOPE_MIGRATE=false`. Only the release-specific Job runs `server migrate`. Reapplying an already completed release is safe; migration history and credentials are preserved.

Notification delivery is disabled by default. To activate it, set `OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED=true` before rendering. Optional bounded inputs are `OKOSCOPE_NOTIFICATION_POLL_MS`, `OKOSCOPE_NOTIFICATION_CLAIM_SIZE`, `OKOSCOPE_NOTIFICATION_CONCURRENCY`, `OKOSCOPE_NOTIFICATION_LEASE_SECONDS`, `OKOSCOPE_WEBHOOK_TIMEOUT_SECONDS`, `OKOSCOPE_WEBHOOK_MAX_ATTEMPTS`, `OKOSCOPE_WEBHOOK_BACKOFF_MIN_SECONDS`, `OKOSCOPE_WEBHOOK_BACKOFF_MAX_SECONDS`, `OKOSCOPE_WEBHOOK_MAX_RESPONSE_BYTES`, and `OKOSCOPE_NOTIFICATION_DRAIN_SECONDS`. Invalid values fail rendering; an absent, malformed, or all-zero encryption key fails the cluster check. Output contains only activation state, bounds, and enabled destination count.

Before activation, review enabled destinations through the tenant-scoped API, confirm each receiver deduplicates by stable delivery ID, and verify it validates the timestamped HMAC signature. `okoscope_notification_worker_state` is a bounded gauge: `0=disabled`, `1=ready/idle`, `2=backlogged`, `3=retrying`, `4=failing`, and `5=draining`. Alert on oldest due work, due/retrying deliveries, terminal failures, expired leases, cycle failures, and drain timeouts. Receiver failures are delivery signals and do not make the core API unready.

Authenticated users can read the equivalent Project-scoped snapshot from `GET /api/v1/projects/{project_id}/notification-health`. The response uses the string states `disabled`, `idle`, `backlogged`, `retrying`, `failing`, and `draining`; includes only bounded counts, nullable oldest-due age, and an observation timestamp; and never returns destination URLs or secret material. The endpoint is intended for 10-second UI polling and uses `Cache-Control: no-store`. Notification failure changes this snapshot but does not make the ingestion/API readiness probe fail.

To pause delivery, render and roll out with `OKOSCOPE_NOTIFICATION_DELIVERY_ENABLED=false`. Workers stop taking new claims and drain in-flight work for the configured bounded interval; queued and retryable rows remain durable. Re-enable with the same key to resume. Back up the encryption key separately from the database. Rotation must use the destination rotation API so stored secrets are re-encrypted deliberately. Never delete delivery rows, outbox rows, or migrations as rollback.

For public routing, set `OKOSCOPE_DOMAIN`, `OKOSCOPE_CERTIFICATE_NAME`, `OKOSCOPE_CERT_ISSUER`, `OKOSCOPE_TLS_SECRET`, `OKOSCOPE_HTTP_ENTRYPOINT`, `OKOSCOPE_HTTPS_ENTRYPOINT`, `OKOSCOPE_SERVER_SERVICE`, and `OKOSCOPE_WEB_SERVICE`, then render with the final argument `enabled`. Existing environments must use their current Certificate name and issuer to avoid competing ownership of one TLS Secret. Invalid or missing values fail before rendering.

## Verification and rollback

After rollout, verify `/readyz`, `/api/v1/build-info`, server migration logs, connected agent sessions, Certificate readiness, the HTTPS redirect, `/api` routing, Web fallback, and one bounded runtime-event smoke. Record the image IDs and database migration from `PROVENANCE.txt`.

If application readiness fails after a successful additive migration, render `03-upgrade.yaml` with the previous compatible image commits and apply it. Do not delete or roll back migration rows, Jobs, the Secret, StatefulSet, or PVC. If the previous server is not forward-compatible with the recorded migration, stop rather than forcing rollback.

## PostgreSQL durability

Bundled PostgreSQL is single-replica and is not highly available. A PVC protects against Pod replacement, not operator deletion, storage failure, or corruption. Schedule logical backups with `pg_dump`, encrypt and store them outside the cluster, and regularly test restore into a separate database. Snapshot support is storage-provider-specific. Internet-facing or production installations should use a managed/external PostgreSQL service with automated backups, point-in-time recovery, monitoring, and a documented recovery objective.

## Legacy artifact transition

`deploy/kubernetes/mvp.yaml` is deprecated. It embeds development credentials, runs startup migrations, and combines PostgreSQL with stateless upgrades. It remains for one release only to help compare existing resources; do not use it for new installs or upgrades. Recovery during the transition uses the last known immutable application images through `03-upgrade.yaml`, never the monolithic manifest.
# Notification recovery and retention

Notification recovery commands preserve the stable delivery identifier and prior attempts. A manual retry increments `recovery_generation`, resets only the current generation's attempt budget, and remains subject to the normal worker concurrency and signing path. Pending work can be cancelled, but an unexpired in-flight lease returns a conflict because Okoscope cannot prove that interrupting the local request prevents receiver processing.

Every mutating recovery request requires an `Idempotency-Key`. The server stores only a keyed hash and a canonical request fingerprint; raw keys, bearer credentials, signing secrets, destination URLs, and payload bodies are excluded from operation audit records and logs. Bulk retry is capped at 200 deliveries per confirmed command and reports whether eligible work remains.

Terminal history retention is disabled by default. Configure it explicitly with:

- `OKOSCOPE_NOTIFICATION_RETENTION_ENABLED`;
- `OKOSCOPE_NOTIFICATION_TERMINAL_RETENTION_DAYS` (1–3650, default 90);
- `OKOSCOPE_NOTIFICATION_RECOVERY_RETENTION_DAYS` (1–3650, default 365);
- `OKOSCOPE_NOTIFICATION_RETENTION_BATCH_SIZE` (1–10000, default 1000);
- `OKOSCOPE_NOTIFICATION_RETENTION_POLL_SECONDS` (60–86400, default 3600).

The maintenance loop deletes only terminal deliveries older than the boundary and preserves pending, retryable, and in-flight work. `server notification-retention` runs one independently callable bounded batch. Deletions are irreversible without a PostgreSQL backup; validate retention with representative volume before production activation.
