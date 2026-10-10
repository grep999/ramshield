# Lockdown and recovery runbook

This runbook covers the host controls that protect RamShield's control plane and
SYNPROXY deployment. Apply changes during a maintenance window and keep an
out-of-band console available for network changes.

## 1. Apply SYNPROXY prerequisites

The daemon runs as the dedicated `ramshield` user. It does not write kernel
tunables. Install the checked-in profile before enabling SYNPROXY:

```sh
sudo install -D -m 0644 deploy/sysctl.d/99-ramshield-synproxy.conf \
  /etc/sysctl.d/99-ramshield-synproxy.conf
sudo sysctl --system
sysctl net.ipv4.tcp_syncookies net.ipv4.tcp_timestamps net.netfilter.nf_conntrack_tcp_loose
```

Expected values are `1`, `1`, and `0`, respectively. Strict conntrack mode
can affect asymmetric or unusual routing; validate the host traffic path first.
The daemon checks these values before applying its nftables rules and reports a
configuration error if they do not match.

## 2. Install the service sandbox

```sh
sudo install -D -m 0644 deploy/systemd/ramshield.service \
  /etc/systemd/system/ramshield.service
sudo systemctl daemon-reload
sudo systemd-analyze verify /etc/systemd/system/ramshield.service
sudo systemctl restart ramshield
sudo systemctl --no-pager --full status ramshield
```

The service uses `ProtectKernelTunables=true`; sysctl writes are performed by
the system boot configuration, not by the daemon. Keep WAL, SHM, and pinned BPF
write paths restricted to the locations declared in the unit.

## 3. Verify health before exposing control surfaces

```sh
systemctl is-active --quiet ramshield
curl --fail --silent http://127.0.0.1:9999/healthz
journalctl -u ramshield --since '-10 min' --no-pager
```

Confirm the expected IPC/dashboard bind addresses and authentication settings
in `/etc/ramshield/config.toml`. Do not expose control-plane endpoints directly
to an untrusted network.

## 4. Roll back a failed deployment

1. Stop the daemon: `sudo systemctl stop ramshield`.
2. Restore the last known-good service/config files.
3. If SYNPROXY was enabled, inspect `nft list table inet ramshield_synproxy`
   before removing rules; preserve unrelated administrator rules.
4. Restore the previous sysctl profile if the host requires different values,
   then run `sudo sysctl --system`.
5. Start RamShield and verify health, logs, and dataplane projection status.

Do not delete WAL/checkpoint files during routine rollback; they are recovery
state, not disposable runtime cache.
