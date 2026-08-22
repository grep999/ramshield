# Research Tickets (Pulse-friendly)

## TICKET: Q5 Kubernetes Operator spike
- **task_id:** roadmap/Q5-Kubernetes-Operator
- **title:** Spike: RamShieldCluster CRD + kube-rs controller for auto-scaling
- **priority:** high (Q5, zero coverage before this run)
- **effort:** M (new `src/operator/` module + CRD yaml)
- **research:** https://kube.rs/controllers/intro/ | https://docs.rs/kube/latest/kube/ | https://kubernetes.io/docs/concepts/extend-kubernetes/operator/
- **acceptance:** controller reconciles a `RamShieldCluster` CR; scales replicas from dashboard `/metrics` RPS/RAM headroom; clippy clean.
