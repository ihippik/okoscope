# Install Okoscope on Kubernetes

Okoscope publishes two charts with the same semantic release version:

- `oci://ghcr.io/ihippik/charts/okoscope-agent` connects a cluster to an existing hosted or self-hosted server.
- `oci://ghcr.io/ihippik/charts/okoscope` installs server, Web, migrations, and optionally the local agent. It never installs PostgreSQL.

Production commands should always include `--version <OKOSCOPE_VERSION>`. Component images are pinned by the chart release. Never put a database URL or Application credential in a Helm values file or `--set` argument.

## Connect Kubernetes to Okoscope

Create an Application in Okoscope and copy its one-time `oko_app_v1_...` credential into a Kubernetes Secret without writing it to disk:

```bash
kubectl create namespace okoscope-system
read -rsp 'Application credential: ' OKOSCOPE_APPLICATION_TOKEN; printf '\n'
kubectl -n okoscope-system create secret generic okoscope-application-credentials \
  --from-literal=payment-api="$OKOSCOPE_APPLICATION_TOKEN"
unset OKOSCOPE_APPLICATION_TOKEN
```

Create `agent-values.yaml` containing only non-secret configuration:

```yaml
server:
  endpoint: https://grpc.okoscope.example:443
identity:
  clusterName: production
workloads:
  - namespace: production
    kind: Deployment
    name: payment-api
    credentialSecret:
      name: okoscope-application-credentials
      key: payment-api
```

Install and verify:

```bash
helm upgrade --install okoscope-agent \
  oci://ghcr.io/ihippik/charts/okoscope-agent \
  --version <OKOSCOPE_VERSION> \
  --namespace okoscope-system \
  -f agent-values.yaml
kubectl rollout status daemonset/okoscope-agent-okoscope-agent \
  --namespace okoscope-system --timeout=5m
```

For a private CA, create an additional Secret and configure `server.caSecret.name` and `server.caSecret.key`. System trust is used when `caSecret.name` is empty. Plaintext transport is an explicitly isolated development mode only: use an `http://` endpoint together with `server.developmentPlaintext=true`; never use it across untrusted networks.

Use `labels` instead of `name` for a bounded label selector. Do not set both. Multiple mappings may use different Secret names and keys. The chart grants read-only access to Pods, Deployments, ReplicaSets, and the `kube-system` Namespace, and requires Linux eBPF support described in [platform support](platform-support.md).

If the DaemonSet is not ready, inspect `kubectl logs -n okoscope-system daemonset/okoscope-agent-okoscope-agent`. Common causes are an unreachable TLS endpoint, an incorrect CA, unsupported kernel/BTF support, or a missing Secret key.

## Self-host Okoscope

Prerequisites are Kubernetes, Helm 3, and an existing supported PostgreSQL database reachable from the target namespace. The database and its availability, TLS, security, capacity, backup, restore, and upgrade lifecycle remain the user's responsibility.

Create the database Secret safely:

```bash
kubectl create namespace okoscope-system
read -rsp 'PostgreSQL connection URL: ' OKOSCOPE_DATABASE_URL; printf '\n'
kubectl -n okoscope-system create secret generic okoscope-database \
  --from-literal=database-url="$OKOSCOPE_DATABASE_URL"
unset OKOSCOPE_DATABASE_URL
```

Install and verify:

```bash
helm upgrade --install okoscope \
  oci://ghcr.io/ihippik/charts/okoscope \
  --version <OKOSCOPE_VERSION> \
  --namespace okoscope-system
kubectl rollout status deployment/okoscope-server \
  --namespace okoscope-system --timeout=5m
helm test okoscope --namespace okoscope-system
kubectl port-forward -n okoscope-system service/okoscope-web 8080:80
```

Open `http://127.0.0.1:8080`. The pre-install/pre-upgrade migration Job must succeed before application rollout. It reads `database.existingSecret`/`database.urlKey`; there is deliberately no `database.url` value.

Fresh internal installs enable registration only to create the first Organization owner. Use the private port-forward, create that owner, and immediately close registration while preserving the installed values:

```bash
helm upgrade okoscope oci://ghcr.io/ihippik/charts/okoscope \
  --version <OKOSCOPE_VERSION> --namespace okoscope-system \
  --reuse-values --set server.registrationEnabled=false
```

The chart rejects public Web ingress while registration is enabled.


See [production installation and operations](self-hosted-deployment.md) for ingress-nginx and Traefik TLS examples, external internal Secrets, upgrades, rollback, uninstall, private registries, notifications, and Kustomize transition.
