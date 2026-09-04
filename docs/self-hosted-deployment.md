# Production self-hosted deployment

Start with the [installation quick starts](installation.md). Helm is the supported public interface. Okoscope requires an existing PostgreSQL database and never creates, upgrades, backs up, restores, or deletes database infrastructure or storage.

## Production values

Use separate TLS hostnames for HTTP and gRPC. The first release gates examples for ingress-nginx and Traefik; other controllers require operator verification.

```yaml
database:
  existingSecret: production-database
  urlKey: connection-url
internalSecret:
  existingSecret: okoscope-internal-production
server:
  replicas: 2
  registrationEnabled: false
web:
  replicas: 2
notifications:
  enabled: true
ingress:
  web:
    enabled: true
    className: nginx
    host: okoscope.example.com
    tlsSecret: okoscope-web-tls
  grpc:
    enabled: true
    className: nginx
    host: grpc.okoscope.example.com
    tlsSecret: okoscope-grpc-tls
```

The chart supplies `nginx.ingress.kubernetes.io/backend-protocol: GRPC` for the nginx gRPC route. With `className: traefik`, it supplies the Traefik h2c service annotation. TLS terminates at the Ingress while the server uses cluster-internal plaintext. To have cert-manager create `Certificate` resources, set `certManager.enabled=true` and `certManager.clusterIssuer`; otherwise pre-create the TLS Secrets.

The Web container receives `OKOSCOPE_API_BASE_URL=/` and `OKOSCOPE_API_UPSTREAM=http://<release>-server:8080` from the chart, and proxies exact `/api` and `/api/*` requests to that internal Server Service while preserving their URI. Web ingress and `service/okoscope-web` port-forwarding therefore serve the UI and browser API together; the Server HTTP Service does not need separate public exposure.

For externally managed internal keys, the referenced Secret must contain `admin-credential`, `webhook-encryption-key`, and `identity-token-key`, or the alternative key names configured below `internalSecret`. Leaving `internalSecret.existingSecret` empty lets Helm generate them once with `lookup`; the Secret has a keep policy and values are reused on upgrades. Offline GitOps rendering must use an externally managed Secret because `lookup` cannot recover live state.

Set `imagePullSecrets` for a private registry. Resource requests and limits live under `server.resources`, `web.resources`, and, when enabled, `okoscope-agent.resources`. Notifications are disabled by default and are enabled with `notifications.enabled=true`; the webhook encryption key must remain stable and separately recoverable.

The optional local agent uses the same values contract as the standalone chart below `okoscope-agent`. It still requires an existing Application credential Secret and at least one workload mapping.

## Upgrades and migrations

Pin the same semantic version for both charts:

```bash
helm upgrade okoscope oci://ghcr.io/ihippik/charts/okoscope \
  --version <NEW_OKOSCOPE_VERSION> \
  --namespace okoscope-system \
  -f production-values.yaml \
  --wait --timeout 10m
```

The idempotent migration hook runs before install and upgrade with bounded retry and deadline. A failure stops rollout. Inspect it with `kubectl get jobs,pods -n okoscope-system -l app.kubernetes.io/component=migration` and its Pod logs, correct database connectivity/permissions, then repeat the same `helm upgrade`. Never edit migration rows or attempt to reverse a schema migration.

Use `helm rollback okoscope <REVISION> -n okoscope-system` only when the prior server version is forward-compatible with the applied database migration. Database backups and point-in-time recovery must be managed and tested outside Okoscope.

## Uninstall ownership

`helm uninstall okoscope -n okoscope-system` deletes chart-owned stateless resources. It does not delete the existing database Secret, external PostgreSQL, externally managed credentials, or externally managed TLS/CA Secrets. A chart-generated internal Secret is retained by policy and must be deliberately removed by the operator only after confirming it is no longer required.

## Existing Kustomize installations

Fresh Helm installs are supported in the first release. Automatic adoption of existing Kustomize resources is not. Keep the database and Secrets, back them up, render the new chart with `helm template`, and compare names/selectors before a planned clean migration. Do not install Helm over identically named live resources without first resolving ownership metadata.

The `deploy/kubernetes` Kustomize roots and bundled PostgreSQL manifests are internal/legacy during one compatibility window. They remain available to existing operators but are not a new-install contract. PostgreSQL manifests there must not be used as part of a new Okoscope installation.

## Release and cluster verification

Charts are published as `oci://ghcr.io/ihippik/charts/okoscope` and `oci://ghcr.io/ihippik/charts/okoscope-agent` with shared semantic versions. A release supplies verified immutable server, agent, and Web inputs and records the server's required migration. Publication must wait for chart policy tests and component availability.

Repository release-candidate verification uses the `aliens` context:

```bash
kubectx aliens
helm template okoscope deploy/helm/okoscope -f production-values.yaml
kubectl rollout status deployment/okoscope-server -n okoscope-system --timeout=5m
```

Also verify `/readyz`, `/api/v1/build-info`, required migration readiness, Web/API routing, TLS gRPC connectivity, agent authentication, workload matching, and one bounded runtime event. These checks must never mutate or replace the user-owned PostgreSQL lifecycle.
