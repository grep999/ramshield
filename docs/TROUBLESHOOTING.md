# Troubleshooting

Start with the symptom, then check the component that owns it.

## RamShield will not start

Check the actual error:

```bash
RUST_LOG=ramshield=debug ./target/release/ramshield --config config.toml
# Read-only diagnostics: which gate is failing (config, privileges, XDP, maps, stats, mesh)
./target/release/ramshield doctor --config config.toml
```

Common causes:

- invalid TOML or configuration values;
- public IPC/dashboard bind without the required credentials/boundary;
- missing WAL directory or permissions;
- XDP configuration/interface problems;
- an unknown command-line argument.

The daemon intentionally fails on unknown flags rather than silently ignoring them.

## Health is 503 / degraded

Check:

```bash
curl http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status
```

Look at the returned `reason` and recent logs.

Also check memory:

```bash
curl http://127.0.0.1:9999/api/snapshot | jq '.ram_pct, .memory_usage_mb, .ips_tracked'
```

A high store-memory percentage can move the process into an unhealthy state.

## XDP is inactive

Check:

```bash
./target/release/ramshield-cli status
getcap target/release/ramshield
ip link show
bpftool net
uname -r
```

Then confirm:

- `[xdp].enabled = true`;
- the configured interface exists;
- the binary has the required capabilities;
- the binary was built with the `full` feature.

If `/healthz` returns 503 with `protection_state: failed` and `xdp_configured: true, xdp_active: false`, XDP attach failed at boot. Fix capabilities/interface, or set `[xdp].allow_inband_fallback = true` to start degraded and debug XDP separately.

Re-apply capabilities after every rebuild:

```bash
sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf+eip' target/release/ramshield
```

If XDP still cannot attach, RamShield can fall back to in-band enforcement. That means the daemon may remain usable, but kernel-level packet drops are not active.

## Blocks are not appearing

Check:

```bash
./target/release/ramshield-cli stats
curl http://127.0.0.1:9999/metrics | grep ramshield_
```

Then verify:

- the proxy is sending events to the configured IPC address;
- the configured detection thresholds are appropriate;
- `detection.batch_block_enabled` is true if automatic batch blocking is expected;
- the enforcement queue is not under pressure.

The current enforcement queue is bounded. Queue pressure is surfaced through metrics/errors; do not assume a successful detection decision automatically means an applied XDP block.

## Blocks do not expire

Check the block state:

```bash
./target/release/ramshield-cli check <ip>
./target/release/ramshield-cli info <ip>
```

Then inspect logs for TTL/enforcement errors.

Do not delete the WAL as a first response. Doing so can destroy the persisted history that is needed to understand a recovery problem.

## Restart did not restore an expected block

Check the configured WAL:

```bash
grep -nA8 '^\[wal\]' config.toml
ls -la /var/lib/ramshield/wal
df -h /var/lib/ramshield/wal
```

Then restart with debug logging and inspect WAL replay messages.

Remember:

- WAL must be enabled for restart persistence;
- expired blocks are intentionally not resurrected;
- a WAL/open/replay failure is different from a healthy, durable restart.

## IPC authentication fails

Verify:

- the client uses the correct key;
- the key is valid hexadecimal;
- the key ID is configured;
- the server requires the expected role;
- the client clock is sane for replay protection.

Do not expose an unauthenticated IPC listener on a non-loopback network.

## Dashboard login fails

Check:

```bash
curl http://127.0.0.1:9999/healthz
```

Then verify `dashboard.admin_password_hash` and the bind/HTTPS setup.

RamShield does not provide built-in TLS. A network-exposed dashboard needs an external TLS boundary.

## High CPU

Inspect:

```bash
top
curl http://127.0.0.1:9999/api/snapshot | jq '.cpu_usage, .events_ingested, .channel_depth'
```

Review event volume and detection batching before changing thresholds.

Do not tune the system from a single CPU sample.

## High memory

Inspect:

```bash
curl http://127.0.0.1:9999/api/snapshot | jq '.ram_pct, .memory_usage_mb, .ips_tracked, .ram_limit_mb'
```

Then review `engine.ram_limit_mb`.

A larger limit is not automatically safer. The limit exists to keep the store bounded.

## Last resort

Preserve:

- the config actually used;
- recent logs;
- `/api/snapshot` output;
- relevant metrics;
- WAL metadata/state;
- exact RamShield version/commit.

Then reproduce on a staging host before deleting state or changing multiple settings at once.