# RamShield 30-Minute Production Trial

Install RamShield and have it actively mitigating attacks in under 30 minutes.

## Prerequisites

- Linux host (x86_64 or arm64)
- `curl`, `bash`, `python3`, `systemd`
- Root or sudo access

No kernel capabilities or XDP setup needed — safe mode runs entirely in userspace.

## Step 1: One-Command Install (2 minutes)

```bash
curl -fsSL https://ramshield.io/install.sh | bash
```

This downloads the latest binary, creates a system user, writes a safe-mode
config (`/etc/ramshield/config.toml` with XDP disabled), and installs a
systemd service. Verify:

```bash
/usr/local/bin/ramshield --version
systemctl status ramshield.service
```

If you are offline or building from source:

```bash
cargo build --release --features full
cp target/release/ramshield /usr/local/bin/
cp config-min.toml /etc/ramshield/config.toml
```

## Step 2: Run a Config Sanity Check (1 minute)

RamShield parses its config at startup. Bad config means immediate failure
rather than silent misconfiguration:

```bash
ramshield --config /etc/ramshield/config.toml --validate
```

Expected output: `config validation passed`

If you see errors, fix the highlighted section and re-run.

## Step 3: Start the Service (2 minutes)

```bash
systemctl enable --now ramshield.service
journalctl -u ramshield.service -f
```

You should see startup logs confirming:

- WAL segment created
- Store initialized
- Bucket quantization active
- Listening on IPC port 7890
- Listening on dashboard port 9999 (optional)

## Step 4: Send Traffic Through Your Reverse Proxy (5 minutes)

RamShield is an out-of-band observer/enforcer — it is NOT an HTTP reverse
proxy. Route application traffic through your reverse proxy to YOUR
APPLICATION backend (e.g. `127.0.0.1:8080`), never to RamShield's IPC port
(7890, which speaks newline-delimited JSON and rejects HTTP).

Templates for Nginx, Caddy, HAProxy, and Envoy live in `configs/templates/`;
each points at an application-backend placeholder you must edit.

### Nginx example

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;  # your application backend
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}
```

### Feeding telemetry to RamShield

The proxy (or a sidecar/agent) reports connections to RamShield's IPC port
(7890) using `ramshield_protocol::Message` frames — newline-delimited JSON,
e.g. `report_connections`. Alternatively enable native capture with
`native_ingest.enabled = true` in your config. See `docs/OPERATIONS.md` and
`crates/ramshield-protocol` for the message schema.

## Step 5: Trigger a Block and Verify It Works (10 minutes)

RamShield blocks IPs that exceed `rps_threshold` (default: 500 events/sec).
Trigger a simulated attack to verify detection + block:

```bash
# From an external host or a different IP
for i in $(seq 1 1000); do curl -s -o /dev/null http://target-host/; done &
```

Watch RamShield detect and block:

```bash
journalctl -u ramshield.service -f | grep -i breach
```

Check that the IP is now blocked:

```bash
# The blocked IP should now receive 403 instead of 200
curl -s -o /dev/null -w "%{http_code}" http://target-host/
# Should return 403
```

## Step 6: Verify Observability (5 minutes)

RamShield exposes metrics on the dashboard port (default 9999) at `/metrics`
(Prometheus format). Check:

```bash
curl -s http://localhost:9999/metrics | grep -E "adaptive_breach|numerical_calculation_failure|out_of_order_sample"
```

You should see counters for:

- `adaptive_breach_total` — breaches triggered by adaptive CUSUM
- `numerical_calculation_failure_total` — calculation failures (should be 0)
- `out_of_order_sample_dropped_total` — out-of-order timestamps dropped
- `baseline_frozen_ips_gauge` — IPs currently with frozen baselines

## Step 7: Unblock an IP (2 minutes)

Blocks expire after `block_ttl_secs` (default: 300s / 5 minutes). Or remove
manually via the enforcement API:

```bash
curl -X POST http://localhost:7890/enforce/unblock \
  -H "Content-Type: application/json" \
  -d '{"ip": "203.0.113.10"}'
```

## Step 8: Production Hardening (5 minutes)

Before going to production:

1. **Generate new auth keys** — replace the development keys in
   `/etc/ramshield/config.toml` with random 256-bit hex keys:

   ```bash
   od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
   ```

2. **Enable XDP** — for full throughput, set `xdp.enabled = true` in
   config.toml and grant `CAP_NET_ADMIN` to the binary:

   ```bash
   setcap cap_net_admin+ep /usr/local/bin/ramshield
   ```

3. **Tune thresholds** — adjust `detection.rps_threshold` based on your
   baseline traffic volume (default 500 pps is conservative).

4. **Set up log rotation** — WAL segments grow indefinitely; configure
   `retention_max_bytes` or external log shipping.

## Timeline Summary

| Step | Time | Action |
|------|------|--------|
| Install | 2 min | One-command installer |
| Validate | 1 min | Config sanity check |
| Start | 2 min | systemd service |
| Traffic | 5 min | Route app traffic through RamShield |
| Block test | 10 min | Trigger attack, verify block |
| Observability | 5 min | Check metrics endpoint |
| Unblock | 2 min | Manual IP unblock |
| Harden | 5 min | Auth keys, XDP, tuning |
| **Total** | **~32 min** | **Working production defense** |

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `permission denied` binding to port 7890 | Run `ramshield` as the `ramshield` user or adjust port |
| `WAL segment creation failed` | Ensure `/var/lib/ramshield/wal` is writable by `ramshield` user |
| Traffic returns 502 | RamShield not running; check `systemctl status ramshield` |
| No blocks detected | Lower `detection.rps_threshold` or increase traffic volume |
| `CAP_NET_ADMIN` missing (XDP mode) | `setcap cap_net_admin+ep /usr/local/bin/ramshield` |

## Next Steps

- Read `AGENTS.md` for architecture details
- Review `config.baseline.toml` for all tunable parameters
- Set up Prometheus scraping on the dashboard port (default 9999, `/metrics`)
- Subscribe to the security mailing list for CVE notifications
