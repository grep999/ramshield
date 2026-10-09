# Operations

Daily operator commands and the small set of checks that matter when RamShield is running.

## Check the process and protection state

Health endpoint:

```bash
curl http://127.0.0.1:9999/healthz
```

CLI status:

```bash
ramshield-cli status
```

JSON status:

```bash
ramshield-cli status --json
```

The important distinction is:

```text
process running != kernel enforcement active
```

When XDP is enabled, check the reported XDP state before treating the host as kernel-enforced.

## Inspect traffic and blocks

```bash
ramshield-cli stats
ramshield-cli check <ip>
ramshield-cli info <ip>
```

List active blocks through the dashboard API:

```bash
curl http://127.0.0.1:9999/api/blocks/active
```

Block and unblock manually:

```bash
ramshield-cli block <ip> --reason manual --ttl 300
ramshield-cli unblock <ip>
ramshield-cli unblock-cidr <cidr>
```

Use manual blocks deliberately. They enter the same enforcement path as other block commands.

## Inspect the dashboard API

The dashboard currently exposes:

```text
/healthz
/metrics
/api/snapshot
/api/stream
/api/history/batches
/api/history/blocks
/api/blocks/active
/api/traffic/subnets
/api/status/modules
/api/config
```

Authenticated dashboard routes should be treated as administrative interfaces.

## Logs

Run the daemon with a normal info level:

```bash
RUST_LOG=ramshield=info ./target/release/ramshield --config config.toml
```

For debugging:

```bash
RUST_LOG=ramshield=debug ./target/release/ramshield --config config.toml
```

Look for:

- XDP load/attach failures;
- WAL open/replay failures;
- enforcement queue pressure;
- detection errors;
- shutdown messages;
- authentication or authorization failures.

## SYNPROXY host prerequisites

When `synproxy.enabled = true`, RamShield verifies these host settings before
installing nftables rules:

- `net.ipv4.tcp_syncookies = 1`
- `net.ipv4.tcp_timestamps = 1`
- `net.netfilter.nf_conntrack_tcp_loose = 0`

Install and apply the supplied system-level configuration **before** starting
RamShield:

```bash
sudo install -D -m 0644 deploy/sysctl.d/ramshield-synproxy.conf \
  /etc/sysctl.d/90-ramshield-synproxy.conf
sudo sysctl --system
sudo sysctl -n net.ipv4.tcp_syncookies
sudo sysctl -n net.ipv4.tcp_timestamps
sudo sysctl -n net.netfilter.nf_conntrack_tcp_loose
```

The expected output values are `1`, `1`, and `0`, respectively. The
systemd unit is ordered after `systemd-sysctl.service` and uses
`ProtectKernelTunables=true`; the daemon checks these values but does not
write global kernel tunables. Setting `nf_conntrack_tcp_loose=0` changes
host-wide conntrack behavior, so qualify it against the host's other
firewall/conntrack consumers before enabling SYNPROXY on a production host.

If the daemon reports a SYNPROXY sysctl preflight failure, correct the
system-level settings and restart the service. Do not treat a running process
as proof that SYNPROXY rules were installed.

## WAL

Check the configured WAL directory:

```bash
ls -la /var/lib/ramshield/wal
df -h /var/lib/ramshield/wal
```

When WAL is enabled, block state is replayed on startup. A running process does not by itself prove WAL durability.

## Restart

After a restart, verify:

```bash
curl http://127.0.0.1:9999/healthz
ramshield-cli status
```

If persistent blocks matter, also verify the expected block with:

```bash
ramshield-cli check <ip>
```

and inspect the WAL/logs if recovery did not behave as expected.

## Upgrade

For a release upgrade:

1. Read the release entry in `CHANGELOG.md`.
2. Keep the existing config and WAL.
3. Stop the current service/process.
4. Install or build the new version.
5. Validate startup using the normal config.
6. Check health and status.
7. Verify an expected existing block if persistence is required.
8. Only then reopen normal traffic.

Do not assume WAL/config compatibility across releases unless the release notes say so.

## Rollback

Rollback to the previously verified version/config pair.

Keep the previous WAL and logs until the new version is verified or the rollback decision is complete.

After rollback, check health, status, authentication, and representative block/unblock behavior.

## Important operator rule

Do not report "protected" from process liveness alone.

At minimum, verify:

```text
daemon is healthy
XDP is active when kernel enforcement is required
persistence is healthy when restart durability is required
```