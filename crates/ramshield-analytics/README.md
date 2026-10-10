# ramshield-analytics

```
Input Stream → Raw Counters → Analytics Layer → Decision Output
                ↓              ↓
            Count-Min    HyperLogLog
               Sketch                     Welford
                ↓              ↓              ↓
           Heavy Hitters      Cardinality      Dynamic
                                  Estimation        Baseline
```

## Why it exists

Replaces unbounded HashSet<IpAddr> detection with O(1) space analytics. Bounded-memory streaming for real-time threat detection: HyperLogLog cardinality estimation (1024 bytes per IPv6 subnet), decaying Count-Min Sketch heavy-hitter tracking (512 KiB), and exponentially weighted Welford baseline (32 bytes).

## How it works

### SubnetHll
```rust
pub struct SubnetHll {
    registers: [u8; 1024], // Fixed 1024 bytes on stack
}

impl SubnetHll {
    pub fn insert(&mut self, hash: u64); // Update register
    pub fn estimate(&self) -> f64; // Cardinality estimate
}
```

### DecayingCountMinSketch
```rust
pub struct DecayingCountMinSketch {
    counts: Vec<AtomicU64>, // 512 KiB heap
    inserted_since_decay: AtomicU64,
}

impl DecayingCountMinSketch {
    pub fn increment(&self, key: u64); // Increment all rows
    pub fn estimate(&self, key: u64) -> u64; // Heavy hitter count
    pub fn decay(&self); // Halve counters, reset insertion tracker
}
```

### Welford
```rust
pub struct Welford {
    mean: AtomicU64, // Exponential moving average
    variance: AtomicU64, // Exponential variance
    count: AtomicU64,
    alpha: f64, // Decay factor (0.1 default)
}

impl Welford {
    pub fn update(&self, value: f64); // Weighted mean/variance
    pub fn z_score(&self, value: f64) -> f64; // Outlier detection
}
```

## Uniqueness

1. Fixed-size arrays on stack: SubnetHll uses 1024 bytes of stack memory; no heap allocations, zero GC pressure.
2. Atomic counters + decay: CMS uses atomic operations with exponential decay, bounded memory O(1) w.r.t. stream length.
3. Bitwise mean/variance: Welford packs f64 as AtomicU64 bits, minimizing memory footprint (32 bytes total).

## Dependencies

- memmap2 = "0.9" (for ShmRuleEntry, used by enforcement)

## Testing

**ramshield-analytics**: 6 test functions across 3 modules:
- hll.rs: test_hll_bounded_memory (1)
- welford.rs: converge_to_mean, z_score_outlier (2)
- cms.rs: heap_allocated_no_stack_overflow, decay_halves_counts (2)
- lib.rs: No tests

Note: Analytics crate only depends on stdlib and no external crates beyond those required by downstream crates (ramshield-cgnat).
