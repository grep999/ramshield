# Core benchmark baselines

These measurements are a reproducible starting point, not CI gates. The
measured product paths are from A-037's starting tree
(`754f20df46b84f3a397f754d1f783bf1b2e1c02b`); the synthetic native benchmark
probe was added here. Results came from one local machine and are useful only
for same-machine comparisons.

## Reproduction

Use the locked release profile and retain the complete stdout/stderr with the
result SHA and host metadata. Run `cargo bench --bench field_day --locked` for
detection, enforcement, and the 10k-blocked-store latency workload. Run
`cargo bench -p ramwal --bench wal_bench --locked -- --noplot` for WAL append,
durability, and recovery. Run
`cargo test --release --lib native_packet_to_event_baseline -- --ignored --nocapture`
for the native synthetic packet path. For a separate peak-RSS observation,
run `/usr/bin/time -v` on the field-day executable printed by Cargo after the
release artifacts have been built; record `Maximum resident set size` and
state that it includes the benchmark process, not just the simulated store.

Do not run these workloads concurrently. Record `rustc --version --verbose`,
`uname -a`, CPU model/core count, memory availability, power mode, and whether
the host was otherwise loaded. The field-day custom harness reports one timed
interval per workload; repeat the command at least five times and compare the
median of each labeled result. Criterion reports distributions for the WAL
workloads. The native probe reports the median of five 100k-packet samples.

## Initial local observation

Environment: `rustc 1.100.0-nightly (17fd5b8a3 2026-08-28)`, x86_64 Linux
6.8.0-146-generic, Intel Core i7-3687U (2 cores / 4 threads), 16 GiB RAM
(11.1 GiB available when recorded). The host was not isolated and CPU frequency
scaling was enabled. Five field-day runs were executed sequentially; table
entries there are medians of those runs. WAL is one Criterion session, native
is one run of its five-sample probe, and RSS is one direct-run observation.
These values are descriptive only.

| Workload | Result |
|---|---:|
| Detection flush, 4096 events / 128 IPs, promoted (5-run median; range) | 208,883.6 ns (208,124.8–339,043.7) |
| Detection flush, 4096 events / 4096 IPs, cold skip (5-run median; range) | 2,390,398.3 ns (2,263,574.4–2,633,497.3) |
| Detection flush, 4096 events / 10 blocked IPs (5-run median; range) | 150,409.1 ns (146,854.6–164,999.1) |
| Detection flush, 4096 events / 512 IPs, first attack (5-run median; range) | 444,990.7 ns (427,452.5–469,844.4) |
| Detection flush, 32768 events / 512 subnets (5-run median; range) | 28,773,379.3 ns (28,334,651.9–31,055,557.4) |
| Detection aggregate, 4096 events (5-run median; range) | 441,446.7 ns (399,878.7–522,103.2) |
| Enforcement block, WAL off (5-run median; range) | 2,052.9 ns (2,017.7–2,271.3) |
| Enforcement block, WAL GroupCommit (5-run median; range) | 3,409.1 ns (3,258.5–3,641.1) |
| Enforcement duplicate decision (5-run median; range) | 113.9 ns (110.4–127.6) |
| Enforcement unblock (5-run median; range) | 1,743.9 ns (1,729.9–1,975.5) |
| Store enumerate 10k blocked IPs (5-run median; range) | 298,120.4 ns (291,161.6–311,857.0) |
| WAL append, buffered, 1 B / 100 B / 1 KiB / 4 KiB | 123.54 ns / 523.30 ns / 4.2102 µs / 16.252 µs |
| WAL 1 KiB append, Buffered / Explicit / SyncEach / GroupCommit | 4.1387 µs / 4.1629 µs / 6.0445 µs / 4.1682 µs |
| WAL recovery, 1k / 10k records | 3.8114 ms / 37.407 ms |
| Native synthetic packet-to-event, median of 5 × 100k packets | 112.4 ns/packet |
| Field-day process peak RSS (one direct run) | 63,180 KiB |

The promoted flush contains one high observation (339 µs) compared with its
other four runs (208–221 µs), showing remaining host noise within this short
sequence. The 10k-store case measures enumeration latency, not memory
consumption. RSS is a whole-process peak, not an isolated store measurement.
No regression percentage is set from this single host. Before setting a
threshold, collect repeated observations on a stable, identified runner class
and assess its run-to-run variance.

## Measurement boundaries

The native probe calls the actual `process_packet` path with a fixed synthetic
IPv4/TCP SYN frame and a bounded queue. It includes L3/L4 parsing, rate-budget
admission, event creation, and queue insertion. It does not measure AF_PACKET
capture, kernel scheduling, TPACKET_V3 ring traversal, packet loss, or NIC load;
those require a separately controlled privileged host test. The memory value
from `/usr/bin/time -v` is a whole-process peak-RSS observation and is not an
isolated allocation budget. Mesh merge/gossip remains outside this core suite.

## Missing Measurements (to be completed)

### Kernel capture/TPACKET ring behavior
- **Status**: Not measured due to lack of privileged test environment
- **Plan**: Requires controlled privileged host test to measure AF_PACKET capture, kernel scheduling, TPACKET_V3 ring traversal, packet loss, and NIC load
- **Note**: This measurement is explicitly called out as missing in the audit snapshot

### Isolated memory behavior
- **Status**: Not measured as isolated allocation budget
- **Note**: The current RSS measurement is a whole-process peak-RSS observation and is not an isolated store measurement
- **Plan**: Requires memory profiling tools to isolate store memory usage from benchmark process overhead

## Variance Analysis
- **Status**: Field-day variance on this host is too high to justify thresholds
- **Note**: Before setting a threshold, collect repeated observations on a stable, identified runner class and assess its run-to-run variance
- **Plan**: Repeat measurements on a stable runner class to determine acceptable variance thresholds

