# Release Claims Policy — closes F7 (P2)

No RSS, throughput, latency, or drop-rate figure goes public in release
notes, READMEs, or operator docs without a recorded workload. Each claim must
carry:

- exact git SHA, toolchain, build profile, features, config hash
- machine: CPU, RAM, kernel, driver, NIC
- workload: generator, packet sizes/rates, unique-source cardinality, duration
- measured: process RSS, peak RSS, kernel/XDP map memory, page cache
- measured: CPU, latency p50/p99/p999, drops, detection quality, recovery
- repeated runs with raw output attached

Sub-20 ns microbenchmark language stays out of public claims unless a
reproducible benchmark on the actual implementation and target machine
supports it.

Measured baseline so far: CGNAT SHM lookup cost by probe distance
(`scripts/shm_lookup_bench.c`, commit `5a8a32f`).
