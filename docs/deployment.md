# MVP deployment and verification

The bundled manifest is an evaluation deployment. It uses plaintext gRPC and example credentials and MUST NOT be exposed outside an isolated development cluster.

## Build and deploy

### Images from GitHub Container Registry

The `ci` GitHub Actions workflow tests the workspace and builds multi-platform (`linux/amd64`, `linux/arm64`) agent and server images. Pushes to `main`, version tags matching `v*`, and manual runs publish:

- `ghcr.io/<owner>/<repository>-agent:<commit-sha>`
- `ghcr.io/<owner>/<repository>-server:<commit-sha>`

The workflow also publishes the `main`, version-tag, and `latest` aliases when applicable. Deploy immutable commit-SHA tags rather than mutable aliases. Every publishing run provides an `okoscope-mvp-<commit-sha>` artifact containing `okoscope-mvp.yaml` with both image references already rendered; download it from the workflow run and apply it:

```sh
kubectl apply -f okoscope-mvp.yaml
kubectl -n okoscope rollout status deployment/okoscope-server
kubectl -n okoscope rollout status daemonset/okoscope-agent
```

For private GHCR packages, create a pull secret with a token that has `read:packages`, then attach it to both service accounts before rollout:

```sh
kubectl -n okoscope create secret docker-registry ghcr \
  --docker-server=ghcr.io \
  --docker-username='<github-user>' \
  --docker-password='<token-with-read-packages>'
kubectl -n okoscope patch serviceaccount default \
  -p '{"imagePullSecrets":[{"name":"ghcr"}]}'
kubectl -n okoscope patch serviceaccount okoscope-agent \
  -p '{"imagePullSecrets":[{"name":"ghcr"}]}'
kubectl -n okoscope rollout restart deployment/okoscope-server
kubectl -n okoscope rollout restart daemonset/okoscope-agent
```

The job requests `packages: write` for its `GITHUB_TOKEN`. Organization policies may override this; verify **Settings → Actions → General → Workflow permissions** if GHCR returns `permission_denied`.

### Local images

Build `okoscope/server:dev` from `Dockerfile.server` and `okoscope/agent:dev` from `Dockerfile.agent`, load them into the target cluster, then apply:

```sh
kubectl apply -f deploy/kubernetes/mvp.yaml
kubectl apply -f deploy/kubernetes/e2e-workloads.yaml
kubectl -n okoscope rollout status deployment/okoscope-server
kubectl -n okoscope rollout status daemonset/okoscope-agent
```

The agent requires host PID visibility, `/proc`, cgroup v2, tracefs, BTF, and eBPF privileges. The MVP manifest uses `privileged: true`; production hardening must replace it with the smallest capability set validated for the target distribution. The agent can observe process metadata for every workload on its node even though it forwards only configured workloads, so node access and images must be tightly controlled.

Before a cluster deployment, the image can be smoke-tested on a compatible Docker Linux VM. The repository includes a raw agent configuration at `deploy/examples/agent.yaml`; unlike `agent-config.yaml`, it is not wrapped in a ConfigMap. With a valid kubeconfig mounted for client initialization, the following command mounts tracefs, loads both eBPF programs, and attaches `process.exec` to `sched/sched_process_exec` and syscall observation to `raw_syscalls/sys_enter`:

```sh
docker run --rm --privileged --platform linux/amd64 \
  -e KUBECONFIG=/root/.kube/config \
  -v "$HOME/.kube/config:/root/.kube/config:ro" \
  -v "$PWD/deploy/examples/agent.yaml:/etc/okoscope/agent.yaml:ro" \
  -v /dev/null:/var/run/secrets/okoscope/cluster-credential:ro \
  --entrypoint /bin/sh okoscope/agent:dev \
  -c 'mount -t tracefs tracefs /sys/kernel/tracing && exec /usr/local/bin/okoscope-agent'
```

If attachment succeeds, the agent proceeds to server connection attempts. An `attach okoscope_exec` or `attach okoscope_sys_enter` error instead means the kernel-side smoke test failed. This smoke test validates probe loading and attachment only; Kubernetes attribution and PostgreSQL persistence are covered by the acceptance checks below.

For a non-development installation, remove `developmentPlaintext`, issue a server certificate whose SAN contains the service hostname, mount its certificate/key into the server and the CA certificate into agents, and configure `OKOSCOPE_TLS_CERTIFICATE`, `OKOSCOPE_TLS_PRIVATE_KEY`, and `server.caFile`. Rotate the cluster credential stored in the Secret and do not commit its value.

## Acceptance checks

Trigger process execution in the selected and control Deployments:

```sh
kubectl -n okoscope-demo exec deploy/payment-api -- /bin/sh -c true
kubectl -n okoscope-demo exec deploy/control-api -- /bin/sh -c true
```

Trigger the configured `setns` syscall in the selected workload. The call may return `EPERM` in the unprivileged test container, but the `sys_enter` observation still records the attempted syscall:

```sh
kubectl -n okoscope-demo exec deploy/payment-api -- nsenter -t 1 -m true
```

Port-forward PostgreSQL and inspect selected events:

```sh
kubectl -n okoscope port-forward service/postgres 5432:5432
psql postgres://okoscope:okoscope@localhost:5432/okoscope -f deploy/queries/recent-events.sql
```

The query must show a `process.exec` row with `process_command = 'sh'`, namespace `okoscope-demo`, kind `Deployment`, and workload `payment-api`; it must not show `control-api`. A short-lived executable can disappear from `/proc` before userspace enrichment, so the MVP falls back to the kernel `comm` value (`sh`) instead of promising the original `/bin/sh` path. The agent indexes the host cgroup v2 hierarchy by inode so this race does not lose container attribution.

The `nsenter` command must produce a `syscall` row whose payload names `setns`. The attempt returning `EPERM` is expected and platform-specific: the MVP observes syscall entry, not its return value. Agent JSON logs expose filtered, unattributed, unsupported, decode-failed, capacity-dropped, kernel-lost, sent, retried, and acknowledged counters. After the control execution, `filtered` must increase while no `control-api` event is stored.

## Upgrade and rollback

Apply additive migrations before or together with a compatible server, then roll the server before the DaemonSet. Protocol version negotiation rejects incompatible agents explicitly.

Rollback removes or restores the DaemonSet first, then restores the server image. Do not delete the StatefulSet PVC or run reverse/destructive migrations. Database removal is a separate operator-approved action.

Known MVP limits: one tested Linux profile, Deployment owner chains only, in-memory delivery buffer, no raw-event retention policy, no grouping/findings/releases/UI, shared per-cluster credential, and no enforcement.
