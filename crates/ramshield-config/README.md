# ramshield-config

```text
config.toml ──→ Config::load() ──→ Validation ──→ ConfigHandle (ArcSwap)
                                                       │
                          ┌────────────────────────────┤
                          │                            │
                       Engine                      Detection
```

## Why it exists

Seven subsystems share one process. Each has tunable parameters — thresholds, network addresses, memory limits, cryptographic keys. Without a single configuration source, operators edit code or maintain ad-hoc environment variable lists. `ramshield-config` centralizes all parameters into one type-safe, validated, TOML-based configuration that every subsystem reads through the same handle.

## How it works

`Config` is a plain struct with seven nested sub-configs, one per subsystem:

```rust
pub struct Config {
    pub engine: EngineConfig,        // worker threads, RAM limit, shard count
    pub detection: DetectionConfig,  // thresholds, windows, bloom filter size
    pub xdp: XdpConfig,             // interface, mode, kernel program toggles
    pub ipc: IpcConfig,              // TCP address, connection limits, timeouts
    pub forecasting: ForecastingConfig, // HoltWinters params, anomaly z-scores
    pub wal: WalConfig,              // write-ahead log dir, durability mode
    pub dashboard: DashboardConfig,  // HTTP address, session keys, auth hashes
}
```

`Config::load(path)` does four things in sequence:

1. Reads a TOML file into the struct via serde.
2. Applies environment variable overrides (`RAMSHIELD_ENGINE__RAM_LIMIT_MB=256`, double-underscore path notation for nested fields).
3. Runs `validate()` — rejects values that would crash or produce incorrect behavior (RAM limit < 64 MB, batch window = 0, weak HMAC keys, non-HTTPS dashboard on public interfaces).
4. Returns a `ConfigHandle` — an `Arc<ArcSwap<Config>>` — that hot-reload can swap atomically without restarts.

`validate()` catches: RAM limits below 64 MB, zero batch windows, batch thresholds lower than event thresholds, HMAC keys using the built-in placeholder, HTTP-exposed dashboards without TLS, XDP interfaces shorter than two characters.

`exposure_warnings()` flags network-level exposure risks — public-facing dashboard binds, IPC on non-loopback addresses — and returns human-readable strings.

## Uniqueness

**Fail-closed by default.** Every field has a conservative fallback. An empty `config.toml` produces a working system that blocks aggressively, logs loudly, and refuses to bind to public interfaces. No magic — the defaults encode "safer to block too much than too little."

**Env-override paths use TOML key notation.** `RAMSHIELD_DETECTION__RPS_THRESHOLD=50000` maps to `detection.rps_threshold`. No separate env var per field. One naming convention covers all 47 configurable values.

## Dependencies

**Writes to:** nothing. This crate is pure data.

**Read by:** every other crate. `Engine::new()` receives an owned `Config`. Subsystems access it through `ConfigHandle` (the `ArcSwap` wrapper), which allows hot-reload without restart — the detection engine, IPC server, and dashboard all read their sub-configs from the same shared handle.

**Depends on:** `serde`, `toml`, `anyhow` (error handling), `anyhow` (validation errors). No workspace crate dependencies.

## Benchmarks

No hot-path benchmarks — configuration is read once at startup and cached. Hot-reload via `ConfigHandle` is O(1) pointer swap.

## Testing

75 fuzz tests in `tests/fuzz.rs` that randomly mutate config fields and verify validation catches invalid states. The TOML deserialization is tested implicitly through every integration test that loads `config.toml` or `config.stress.toml`.
