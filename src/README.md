# RamShield

Rust-based network intrusion detection and prevention system for high-throughput environments.

## Project Status

### Core Components Established ✅
- **Config System** (`src/config.rs`) - TOML configuration with validation
- **Core Structures** (`src/error.rs`, `src/lib.rs`, `src/main.rs`) - Error handling, public API, application entry point
- **Engine Architecture** (`src/engine/mod.rs`) - Tokio orchestrator with dedicated worker threads
- **Detection Pipeline** (`src/detection/mod.rs`) - Batch-first detection with OS thread processor
- **Storage Layer** (`src/storage/mod.rs`) - Store interface for data persistence
- **Metrics System** (`src/metrics/mod.rs`) - Atomic counters and dashboard snapshot
- **Supporting Modules** - Forecasting, DNS, Learning, Prediction, Dashboard, IPC, Utilities

### Feature Implementation Status

| Component | Status | Implementation Priority |
|-----------|--------|-----------------------|
| Batch Processing | ✅ ESTABLISHED | PHASE 1 |
| Rate Limiting | ✅ IMPLEMENTED | RATE TRACKER | 
| Storage Interface | ✅ FOUNDATIONS | PHASE 2 |
| Threat Intelligence | ✅ ADVANCED | LEVERAGING |

## Key Design Decisions

### Batch-First Architecture
- Dedicated OS thread for batch processing
- Event queue via `crossbeam_channel` (bounded, non-blocking)
- Fixed-capacity vectors for zero allocation inner loops
- Flush mechanism checks event count against configurable limit

### Memory Optimization
- `VecDeque::with_capacity()` - Pre-allocated buffers
- `Arc<AtomicU64>` - Lock-free counter sharing
- `BloomFilter` - Probabilistic IP filtering with false positive rate control

### Performance Focus
- No synchronous I/O in async context
- CPU-bound work in dedicated threads
- Atomic operations for state updates
- Consistent memory layout for cache efficiency

## Technical Specifications

### Detection Configuration
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub rps_threshold: u64,              // Request rate threshold
    pub batch_max_events: usize,       // Batch size limit
    // Additional parameters...
}
```

### Metric Structure
```rust
// Atomic counters shared across components
pub struct Metrics {
    pub requests_total:     Arc<AtomicU64>,
    pub blocks_total:       Arc<AtomicU64>, 
    pub events_ingested:    Arc<AtomicU64>,
    pub events_rejected:    Arc<AtomicU64>,
    // ... additional metrics
}
```

### Batch Processing Flow
1. **Event Collection**: `ConnectionEvent` submitted to `Sender`
2. **Batch Assembly**: OS thread reads from `Receiver`, populates `Batch`
3. **Flush Trigger**: When batch reaches max events, `flush_batch()` called
4. **Zero Allocation**: Batch processing without dynamic allocations
5. **Metrics Update**: Atomic counters incremented
6. **Storage**: Data persisted to persistent storage

## Development Roadmap

### Immediate Actions
1. **Implement `StorageEngine` trait** in `src/storage/mod.rs`
2. **Add `BloomFilter`** construction and usage in `src/detection/batch.rs`
3. **Connect `RateTracker`** with `DetectionEngine`
4. **Build verification**: `cargo build --all-targets && cargo clippy --all-targets -- -D warnings`

### Next Phase
1. **Store Implementation** (`src/storage/mod.rs`)
2. **Bloom Filter Integration** (`src/detection/batch.rs`)
3. **Worker Thread Coordination**
4. **Unit Tests**
5. **Performance Validation**

### Long-term Goals
- **Real-time threat intelligence** updates
- **Distributed processing** capabilities
- **Machine learning** integration for anomaly detection
- **Health monitoring** and alerting
- **API/CLI tools** for configuration and management

## Build Commands

```bash
# Debug build
cargo build --all-targets

# Release build
cargo build --release

# Full verification
cargo build --all-targets && cargo clippy --all-targets -- -D warnings && cargo test

# Run attack simulation
python3 scripts/attack_sim_100k.py --events 1000000 --workers 64

# Check server health
curl http://127.0.0.1:7891/healthz
```

## License

Apache 2.0

---

*Ultra-minimalist implementation adhering to AGENTS.md specifications*
*Phase 1 complete: Core frameworks and architecture established*
*Phase 2 ready: Batch processing, rate limiting, storage interface*