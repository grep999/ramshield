# AuthN/AuthZ + Enterprise-Readiness Plan (pre-worker-research draft)

Status: my scan done; subagent deep research in flight → will merge/adjust.

## Threat model (two planes, two answers)

**IPC plane (:7890)** — event senders, high-throughput. Senders are
deployed by us/integrators; they hold a shared secret. Threat: rogue process on
LAN injecting fake events to poison detection (fake floods → blocks legit IPs)
or spamming past RAM limits.
**Dashboard plane (:9999)** — admin actions (unblock_ip, config via env only
today). Threat: anyone on network lifting blocks / reading block history /
recon of protected ranges.

## Chosen design — lightweight, boring, wire-compatible

### A1. IPC auth: pre-shared API key + HMAC frame tag (S)
- Config: `[auth] ipc_keys = ["keyid:hexsecret", ...]` (multiple keys → key
  rotation without downtime).
- Wire: NEW optional field `auth` on Request envelope — internally-tagged enum
  already has `deny_unknown_fields` but we ADD a named optional field, old
  senders unaffected; server enforces when `[auth]` present in config.
- Per-frame: `auth = {"kid":"k1","mac":"hex"}` where mac =
  HMAC-SHA256(secret, kid || body_bytes) over the exact line bytes. Constant-
  time compare (`subtle`/`constant_time_eq`).
- Why HMAC not just static header: newline-delimited multi-frame connections —
  per-frame MAC kills session hijack/replay-in-connection cheaply. Timestamp
  nonce field optional later (replay window) — YAGNI now, LAN threat.
- Cost: ~60 lines codec+server, zero deps beyond `hmac`+`sha2` (RustCrypto,
  maintained, no_std core). Throughput impact ~1-2% at 40k eps.

### A2. Dashboard auth: session cookie + Argon2 login (S)
- `[dashboard] admin_password_hash` (argon2 PHC string in config/env), single
  admin user — YAGNI: RBAC/multi-user until a customer asks.
- GET /login (tiny inline form), POST /login → verify argon2 → Set-Cookie
  session token (random 32B, stored in-process DashSet w/ 8h TTL).
- axum middleware layer on everything except /healthz + /login + static.
- Constant-time compares everywhere. `argon2`, `rand`, no JWT (no cross-service
  SSO need).

### A3. TLS: defer to reverse proxy (decision, not omission)
- Document: "put nginx/caddy/traefik in front for TLS" — standard appliance
  pattern. Native rustls = cert lifecycle burden we don't want in v1.
- ponytail ceiling: tokio-rustls behind `[tls]` flag if a buyer demands it.

### A4. Audit trail (M)
- Enforcement decisions already append WAL. Add: auth failures (rate-limited
  log line), admin unblock/config-change events → same audit sink with
  actor=ip/session. Tamper-evident = hash-chain the audit file (prev_hash in
  each record) — ~30 lines, no dep.

## Other enterprise gaps (worker will validate/prioritize)

| Item | Effort | v1 blocker? |
|---|---|---|
| Prometheus /metrics endpoint | S | yes |
| systemd unit + distro packaging | S | yes |
| Docker image (multi-stage, distroless) | S | yes |
| cargo-deny + cargo-audit CI gate | S | yes |
| Structured logging (tracing-subscriber json flag) | S | yes |
| Graceful shutdown draining IPC conns | S | partially exists |
| Config validation errors human-readable | S | yes |
| SBOM (cargo auditable build) | S | nice-to-have |
| Signed release artifacts (minisign/sigstore) | M | wait |
| Multi-user RBAC | L | cut |
| OIDC/SSO integration | L | cut |
| HA/clustering | XL | cut |

## Implementation order (after worker merges back)

1. A2 dashboard auth (exposed surface first) + tests
2. A1 IPC HMAC frames + backward-compat test (old clients rejected ONLY when
   [auth] configured)
3. A4 audit hash-chain
4. /metrics endpoint (worker likely confirms)
5. Packaging trio: systemd/docker/deny-CI

Each its own commit, gates green before push, suite re-run after 1+2.
