# ramshield-analytics

## High-throughput streaming analytics with bounded memory.

This crate replaces unbounded `HashSet<IpAddr>` detection with O(1) space data structures designed for production deployment.

## Data Structures

### HyperLogLog (HLL) — Cardinality Estimation per Subnet (IPv6)
- **Memory**: 1024 bytes
- **Purpose**: Tracks unique IP counts per IPv6 subnet
- **Use case**: Subnet-level cardinality estimation without storing individual IPs

### Decaying Count-Min Sketch (CMS) — Heavy-Hitter Frequency Tracking
- **Memory**: 512 KiB (4 rows × 65,536 columns × AtomicU64)
- **Purpose**: Tracks frequency of heavy-hitting IPs / subnets
- **Features**:
  - Decay scheduling via `inserted_since_decay` counter
  - Single shared instance per daemon — O(1) space w.r.t. stream length
  - Used for identifying top-k threat sources

### Exponentially Weighted Welford — Dynamic Baseline
- **Memory**: 32 bytes
- **Purpose**: Tracks mean + variance with exponential decay
- **Use case**: Dynamic baseline adjustment for detection thresholds
- **Alpha parameter**: Controls decay rate (0 < alpha <= 1)

## Integration

These bounded-memory primitives replace unbounded `HashSet<IpAddr>` in the detection pipeline, providing:
- **O(1) space** regardless of event volume
- **Bounded memory** suitable for resource-constrained environments
- **Deterministic convergence** with proper decay scheduling

## Usage

```rust
use ramshield_analytics::{SubnetHll, DecayingCountMinSketch, Welford};

// HLL for IPv6 subnet tracking
let hll = SubnetHll::default();
hll.insert(hash_value);

// Decaying CMS for heavy-hitter tracking
let cms = DecayingCountMinSketch::new();
cms.insert(ip_hash, expiry_ms);

// Welford for dynamic baseline
let welford = Welford::new(alpha);
welford.update(event_value);
```

## Benchmarks

Refer to `/docs/development/BENCHMARK_BASELINES.md` for runtime performance measurements.

## Related Modules

- `crates/ramshield-cgnat` — CGNAT classification and RFC 6598 detection
- `crates/ramshield-detection` — Detection pipeline using these analytics primitives
- `docs/development/BENCHMARK_BASELINES.md` — Benchmark results and thresholds

## License

MIT
