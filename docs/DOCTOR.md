# ramshield doctor

`ramshield doctor` runs read-only diagnostics before deployment. It checks the
effective configuration, Linux kernel and capabilities, the configured
interface/XDP mode, bpffs and BPF map visibility, detector configuration, and
configured federation peer reachability.

## Usage

```bash
sudo ramshield doctor
ramshield doctor --subsystem config --config /etc/ramshield/ramshield.toml
ramshield doctor --subsystem xdp
ramshield doctor --subsystem maps
ramshield doctor --subsystem stats
ramshield doctor --subsystem mesh
```

Subsystems: `core`, `config`, `xdp`, `maps`, `stats`, and `mesh`.
The explicit `--config` argument wins; otherwise doctor checks
`RAMSHIELD_CONFIG`, then `/etc/ramshield/ramshield.toml`,
`/etc/ramshield/config.toml`, `config.toml`, and finally
`config.baseline.toml` when present in a source checkout.

Exit status is `1` if any check fails and `0` when there are no critical
failures. Warnings are informational and do not force a non-zero exit.

## What the results mean

- **Core:** effective UID/capabilities, kernel version (5.15 diagnostic
  baseline), and kernel BTF availability.
- **Config:** uses the same RamShield loader, environment overrides, and
  validation rules as startup. It does not duplicate or weaken schema rules.
- **XDP:** checks interface presence and reads `ip -details link` output. It
  never attaches an XDP program. Driver-mode support cannot be proven without
  a real attach attempt, so the result is a warning until qualified on the
  target host.
- **Maps:** checks whether bpffs is mounted, looks for the conventional
  `/sys/fs/bpf/ramshield` pin directory, and attempts a read-only
  `bpftool map show`. Map occupancy and capacity are not fabricated when
  authoritative handles/telemetry are unavailable.
- **Stats:** validates EWMA/rate-window/batch timing parameters. A standalone
  doctor cannot prove live detector thread heartbeats or internal CUSUM state;
  it reports this limitation explicitly.
- **Mesh:** attempts bounded TCP connections to configured peer socket
  addresses and reports elapsed connect time. TCP reachability is not proof
  of authenticated gossip, acknowledgement, or state convergence.

## Safety boundary

Doctor is diagnostic only: it does not mount bpffs, create map pins, change
network configuration, attach/detach XDP, mutate firewall rules, or send a
mesh mutation. Run it on the target Linux host with the same configuration and
privilege context used by the service. A clean report is not a substitute for
load testing, packet-path verification, or an end-to-end production smoke test.
