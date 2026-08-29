# RamShield on Kubernetes

Minimal, opinion-free manifests to run RamShield as a sidecar-style daemon
next to your reverse proxy. **No operator yet** — these are the
`Deployment`/`Service`/`ConfigMap` basics. A real operator (rolling config
updates, WAL PVC orchestration, XDP-aware scheduling) is on the roadmap.

## Files

| File | Purpose |
|------|---------|
| `namespace.yaml` | `ramshield` namespace |
| `configmap.yaml` | Default `config.toml` (IPC + dashboard bound to `0.0.0.0`) |
| `deployment.yaml` | 1-replica Deployment; mount ConfigMap, WAL on `emptyDir` |
| `service.yaml`    | ClusterIP for IPC (7890) and dashboard (9999) |
| `rbac.yaml`       | ServiceAccount + minimal role for `ConfigMap` read |

## Why no `StatefulSet`?

The WAL currently uses local disk. In a multi-replica deployment each pod
would have its own WAL — fine for in-memory state (DashMap is per-process)
but not for cross-replica durability. Until WAL ships a shared-backend
mode, **run RamShield as a single replica**, possibly with
`PodDisruptionBudget: maxUnavailable=0`.

## Why no operator?

Operators are real engineering work. Until we have:

- a reason to run more than one replica with shared state, **or**
- a frequent need for safe config reload without restart,

a static Deployment is the right tool. Adding a CRD before it's needed is
the kind of scaffolding `ponytail:` notes are for.

## Apply

```bash
# 1) Build & push the image (one-time per release)
docker build -t ghcr.io/grep999/ramshield:0.2.0 .
docker push ghcr.io/grep999/ramshield:0.2.0

# 2) Apply manifests
kubectl apply -f deploy/k8s/

# 3) Reach the dashboard
kubectl -n ramshield port-forward svc/ramshield-dashboard 9999:9999
# Open http://localhost:9999
```

**The container image is not pre-built.** The `Containerfile` at the repo
root produces a distroless nonroot image; the `ghcr.io/grep999/ramshield:0.2.0`
reference in `deployment.yaml` will fail with `ImagePullBackOff` until you
build and push it. CI builds are tracked under issue #128.

## XDP

The Deployment runs **without** XDP (`[xdp] enabled = false`) because K8s
pods don't own host network devices. To enable XDP, run RamShield on a
`hostNetwork: true` node with the `CAP_SYS_ADMIN` + `CAP_NET_ADMIN`
capabilities and a dedicated NIC.
