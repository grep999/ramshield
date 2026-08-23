# Research Tickets (Pulse-friendly)

## TICKET: Q5 Kubernetes Operator spike
- **task_id:** roadmap/Q5-Kubernetes-Operator
- **title:** Spike: RamShieldCluster CRD + kube-rs controller for auto-scaling
- **priority:** high (Q5, zero coverage before this run)
- **effort:** M (new `src/operator/` module + CRD yaml)
- **research:** https://kube.rs/controllers/intro/ | https://docs.rs/kube/latest/kube/ | https://kubernetes.io/docs/concepts/extend-kubernetes/operator/
- **acceptance:** controller reconciles a `RamShieldCluster` CR; scales replicas from dashboard `/metrics` RPS/RAM headroom; clippy clean.

## TICKET: Q4 blog + community hub bootstrap
- **task_id:** roadmap/Q4-Blog-Community-Stack
- **title:** Launch Zola-based weekly blog + enable GitHub Discussions for community feedback
- **priority:** high (Q4 had zero research coverage before this run)
- **effort:** S (no Rust code changes; content + repo config only)
- **research:** https://www.getzola.org/ | https://docs.github.com/en/discussions/quickstart | https://rust-lang.github.io/mdBook/
- **acceptance:** blog builds with `zola build` in CI and deploys to GitHub Pages; Discussions enabled with Q&A/Announcements/Show-and-tell; README links to both; first post drafted from BLOG_CALENDAR.md week 1 topic.

## TICKET: Q5 Cloudflare Workers edge-detection spike
- **task_id:** roadmap/Q5-Cloudflare-Workers-Edge
- **title:** Spike: edge pre-filter Worker (`worker` crate / workers-rs) mirroring rate_tracker EWMA at Cloudflare edge
- **priority:** high (last uncovered Q5 roadmap task)
- **effort:** M (new `edge-worker/` crate targeting wasm32-unknown-unknown; no core src changes)
- **research:** https://developers.cloudflare.com/workers/languages/rust/ | https://docs.rs/worker/latest/worker/ | https://github.com/cloudflare/workers-rs
- **acceptance:** `#[event(fetch)]` handler scores request IPs against EWMA counters stored in Workers KV/Durable Objects; bans forwarded to origin dashboard IPC endpoint; `wrangler deploy` succeeds and spike documented in docs/.
