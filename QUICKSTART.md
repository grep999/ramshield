# RamShield Quick Start

**Canonical instructions:** [docs/QUICKSTART.md](docs/QUICKSTART.md)

That document is the single source of truth for build, first run (no XDP), XDP production setup, CLI usage with IPC auth, and validation.

## One-command install (root)

```bash
curl -fsSL https://raw.githubusercontent.com/grep999/ramshield/master/scripts/install.sh \
  | sudo bash -s -- --version 0.6.0
```

The installer requires an explicit `--version`. It installs the binary, a system user, a safe-mode config (XDP off), and a systemd unit.

After install:

```bash
ramshield --version
systemctl status ramshield.service
ramshield --config /etc/ramshield/config.toml --validate
```

## Userspace / non-root install

```bash
curl -fsSL https://raw.githubusercontent.com/grep999/ramshield/master/scripts/install-user.sh \
  | bash -s -- --version 0.6.0
```

Places the binary under `~/.local/bin` and config/state under `~/.config/ramshield` and `~/.local/share/ramshield`. No XDP.

## Important defaults in `config.baseline.toml`

- `ipc.require_auth = true` and a **public development HMAC key** — replace before any real deployment.
- `xdp.enabled = false` — you must turn it on and set the interface for kernel enforcement.
- `wal.dir = "/tmp/ramshield-wal"` — fine for trials; use a persistent path in production.
- Dashboard and IPC bind to loopback by default.

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) and [docs/QUICKSTART.md](docs/QUICKSTART.md) for the full checklist.
