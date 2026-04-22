# Project: demo

A multi-service development project managed by the **bluetext CLI** (`b` command), which runs services in a local Kubernetes cluster via k3d.

## Project Structure

```
demo/
├── config/
│   ├── bluetext.yaml                   # Stack definitions
│   ├── services/                       # Service kustomize configs (recursive discovery)
│   │   └── <service-id>/
│   │       ├── base/                   # Base manifests (Deployment + Service + Ingress)
│   │       │   └── kustomization.yaml
│   │       ├── metadata.yaml           # Template metadata
│   │       └── <environment>/          # Per-environment overlays
│   │           └── kustomization.yaml
│   ├── apps/                           # App kustomize configs
│   │   └── <app-id>/
│   │       ├── base/                   # Base manifests
│   │       │   └── kustomization.yaml
│   │       └── <environment>/          # Per-environment overlays
│   │           └── kustomization.yaml
│   └── <service-id>/                   # Per-service configuration files
├── code/
│   ├── clients/                        # Shared client libraries (Rust crate)
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── models/                         # Shared business logic (Rust crate)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── entities/               # Data structures with CRUD
│   │       ├── types/                  # Request/response schemas
│   │       └── operations/             # Business logic functions
│   ├── services/                       # Service source code
│   │   └── <service-id>/
│   └── apps/                           # App source code
│       └── <app-id>/
└── .bluetext/                          # Auto-generated cache (gitignored)
```

## Bluetext CLI

The CLI lives at `../bluetext-cli` and is invoked as `b`. Key commands:

```bash
# Cluster
b start                      # Create k3d cluster, build images, start host agent
b start --no-ui              # Skip deploying the control plane UI
b stop                       # Delete the cluster
b restart                    # Stop and start the cluster
b cluster status             # Show pods, services, ingresses
b status                     # Full observability dashboard

# Namespaces
b namespace create <id>
b namespace delete <id>
b namespace list

# Services (start/stop/restart accept multiple IDs)
b service list                              # Discover services from config/services/*/base/
b service start <id>... -n <namespace>      # Deploy to cluster
b service stop <id>... -n <namespace>
b service restart <id>... -n <namespace>
b service add <name>... [--from <path>]     # Add service(s) from template repo
b service dev <id> -n <namespace> -- <cmd>  # Run locally with mirrord proxying
b service logs <id>                         # View pod logs (searches all namespaces)

# Apps (isolated client-facing applications, auto-generated namespace)
b app list                                 # Discover apps from config/apps/*/base/
b app start <id>...                        # Deploy to auto-generated namespace (app-<id>)
b app stop <id>...
b app restart <id>...
b app add <name>... [--from <path>]        # Add app(s) from template repo
b app logs <id>                            # View pod logs

# Stacks (composable service groups defined in config/bluetext.yaml)
b stack start <id> -n <namespace>
b stack stop <id> -n <namespace>
b stack restart <id> -n <namespace>
b stack list

# MCP
b mcp start                  # Start MCP server
b mcp stop                   # Stop MCP server

# Client libraries (shared code in code/clients/)
b client add <name>... [--from <path>]     # Add client library from template (e.g. couchbase)
b client list [--from <path>]              # List available client templates

# Project scaffolding
b create <project-name> [--from <path>]     # Create new project from templates
```

### Templates

`b service add` and `b app add` auto-fetch templates from GitHub (`bluetext-dev/bluetext-templates`). Templates are cached at `~/.cache/bluetext/templates/`. Override the template source with `--from` / `-f`, or set `templates_dir` in `~/.config/bluetext/config.yaml`.

## Service Development Modes

Each service runs in one of three modes, configured via annotations in its k8s manifest:

1. **In-cluster** (default) — Full container runs in k3d with hostPath volume mounts for live code sync.
2. **mirrord** — A pause container stub deploys to the cluster; the service runs locally and mirrord intercepts cluster traffic. Set `bluetext.io/dev-command` annotation on the Deployment.
3. **Host-forward** — A socat proxy pod forwards cluster traffic to the host. Set `bluetext.io/dev-mode: host-forward` and `bluetext.io/dev-command` annotations.

### Port Forwards

Set `bluetext.io/port-forwards` on the Deployment to auto-forward cluster service ports to localhost. Works in **all three dev modes**. Format: `service:localPort:servicePort` (comma-separated). Port-forwards are auto-managed by the host agent — discovered from annotations, spawned when the service starts, and cleaned up on stop.

## Adding a New Service

1. Run `b service add <template-name>` to add from a template, or create manually:
2. Create the service source under `code/services/<service-id>/`.
3. Add a Dockerfile if the service needs a custom image.
4. Create a kustomize structure at `config/services/<service-id>/`:
   - `base/kustomization.yaml` — references deployment.yaml, service.yaml, ingress.yaml
   - `base/deployment.yaml` — `metadata.name` and `app` label must match `<service-id>`. Use hostPath volumes under `/var/mnt/workspace/` for code mounting.
   - `base/service.yaml` — ClusterIP with `targetPort` matching the container port, `port: 80`.
   - `base/ingress.yaml` — Host pattern uses the kustomize overlay's namespace and ingress class patches.
   - `metadata.yaml` — Template metadata (name, description, ports, dev_mode).
   - Per-environment overlays (e.g. `development/kustomization.yaml`) reference `../base` and add patches.

## Adding a New App

Apps are isolated client-facing applications (e.g. Flutter, React Native) that get their own auto-generated namespace (`app-<id>`) and a network policy blocking cluster-internal traffic.

1. Run `b app add <template-name>` to add from a template, or create manually:
2. Create the app source under `code/apps/<app-id>/`.
3. Create a kustomize structure at `config/apps/<app-id>/` with `base/` directory containing `kustomization.yaml`, `deployment.yaml`, `service.yaml`, and `ingress.yaml` (same structure as services). Use hostPath under `/var/mnt/project/code/apps/<app-id>` (rewritten to workspace path at deploy time).
4. App commands (`b app start`, `b app stop`, etc.) do not require a `-n` flag — the namespace is auto-generated as `app-<app-id>`.

### Config File Namespace Templating

K8s manifests use `{{NAMESPACE}}` (replaced by CLI at deploy time). For config files mounted via hostPath that need the namespace, use the `__NAMESPACE__` placeholder with an initContainer:

```yaml
initContainers:
  - name: config-templater
    image: busybox:stable
    command: ['sh', '-c', 'sed "s/__NAMESPACE__/$POD_NAMESPACE/g" /config-template/config.json > /config/config.json']
    env:
      - name: POD_NAMESPACE
        valueFrom:
          fieldRef:
            fieldPath: metadata.namespace
```

## Service Config Conventions

- `imagePullPolicy: Never` — Images are built locally and imported into k3d (sequential imports to avoid OOM).
- Image naming: `bluetext-<service-id>:latest` for custom-built images.
- Cache volumes (e.g. `node_modules`, Rust `target/`) map to `~/.bluetext/cache/<hash>/<service-id>/` via hostPath.
- Security contexts: drop all capabilities, disable privilege escalation, use RuntimeDefault seccomp. Do not set runAsUser/runAsNonRoot.
- For single-node clusters, add `tolerations` for `node-role.kubernetes.io/control-plane`.
- Vite-based services need `allowedHosts: ['.bluetext.localhost']` in vite.config for hot reload through ingress.

## Networking

- Services communicate internally via Kubernetes DNS: `<service>.<namespace>.svc.cluster.local`
- External access via Traefik ingress: `http://<service-id>.<namespace>.bluetext.localhost`
- Control plane UI: `http://bluetext.localhost`
