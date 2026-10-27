# RamShield node DaemonSet — retained capabilities (I-012)

Source of truth for the running container: `deploy/k8s/daemonset.yaml`.

The container drops **ALL** capabilities and adds only the set below.
`hostNetwork: true` is required so XDP attaches to the **host** interface and
so probes to `127.0.0.1` reach the daemon’s loopback bind.

| Capability | Required for | What fails without it |
|------------|--------------|------------------------|
| **NET_ADMIN** | XDP attach via netlink (`IFLA_XDP`) | Program load/attach returns EPERM; no kernel blocklist projection |
| **BPF** | `bpf()` map create/update under `/sys/fs/bpf` | Map creation fails (often misreported as a generic map error) |
| **PERFMON** | `perf_event_open` used by capture/telemetry paths | Probe attach / some telemetry paths fail |
| **NET_RAW** | Native ingest `AF_PACKET` / TPACKET_V3 sockets | `native_ingest.enabled=true` cannot open the capture socket |

## Explicit non-goals

- **No `privileged: true`** — Bidirectional mount propagation was dropped for this reason.
- **No write to `/proc/sys`** from the unit/installer path for production defaults.
- Mesh stays optional and disabled unless its own production gate (I-003) is met.

## Operator check

After deploy, confirm effective caps (example):

```bash
grep CapEff /proc/$(pgrep -n ramshield)/status
# or: capsh --decode=$(grep CapEff /proc/self/status | awk '{print $2}')
```

Rebuilds replace the binary inode and drop file capabilities on non-container
installs; containers rely on this securityContext instead of `setcap` on the
binary.
