# ramshield-config

## Problem

RamShield has 7 subsystems (detection, forecasting, storage, enforcement, IPC, dashboard, XDP) each with tunable parameters. Without a single configuration source, operators would need to manage 7 separate config files, environment variables would collide, and security-critical settings (like auth keys for public binds) could be accidentally omitted. A misconfigured public bind without authentication is an open door.

## How it works

This crate owns a single `Config` struct that is deserialized from TOML and threaded through every other crate. The load order is:

1. **TOML file** → `Config::load(path)` deserializes into the struct.
2. **Environment overrides** → `RAMSHIELD_<SECTION>__<FIELD>` env vars override file values. Double-underscore prevents collisions.
3. **Validation** → `Config::validate()` enforces invariants that would cause crashes or security holes.

### Validation rules (fail-closed)

| Rule | Why |
|------|-----|
| `engine.ram_limit_mb ≥ 128` | Prevents degenerate tiny stores |
| `engine.shard_count` is power of 2 | Required by DashMap shard index modulo |
| `ipc.auth_keys` non-empty on public bind | Prevents open IPC servers |
| `dashboard.admin_password_hash` set on public bind | Prevents open dashboards |
| Public bind without TLS → warning logged | Surfaces exposure at boot |

`is_public_bind()` strips IPv6 brackets and checks the host octets. `0.0.0.0`, `[::]`, `[*]` are public. `127.*`, `::1` are always private.

### Hot-reload

`ConfigHandle` is `Arc<ArcSwap<Config>>`. The engine, IPC server, and dashboard each hold a clone. A file watcher or signal handler can swap the inner config atomically — readers see the new config on their next access with zero locking overhead.

### Config struct hierarchy

```
Config
├── engine:      EngineConfig      (worker threads, RAM limit, shard count)
├── detection:   DetectionConfig   (thresholds, windows, gate toggles)
├── ipc:         IpcConfig         (bind address, auth keys, timeouts)
├── forecasting: ForecastingConfig (EWMA alpha, Holt-Winters params)
├── wal:         WalConfig         (segment size, retention, durability)
├── dashboard:   DashboardConfig   (bind address, login limits)
└── xdp:         XdpConfig         (enabled, interface, mode)
```

## Dependencies

```
ramshield-config
  ← ramshield-types (Durability enum)
  ← serde, toml, arc-swap, anyhow, argon2

No other RamShield crate depends on this crate's internals —
only on its public `Config` type.
```

This crate is the leaf of the dependency tree. Every other crate imports `Config` or `ConfigHandle` from it.

## Key types

```rust
pub type ConfigHandle = Arc<ArcSwap<Config>>;

pub struct Config {
    pub engine: EngineConfig,
    pub detection: DetectionConfig,
    pub ipc: IpcConfig,
    pub forecasting: ForecastingConfig,
    pub wal: WalConfig,
    pub dashboard: DashboardConfig,
    pub xdp: XdpConfig,
}

impl Config {
    pub fn load(path: &str) -> Result<Self>        // TOML + env + validate
    pub fn apply_env_overrides(&mut self)           // RAMSHIELD_* env vars
    pub fn validate(&self) -> Result<()>            // fail-closed guards
    pub fn exposure_warnings(&self) -> Vec<String>  // TLS-less bind warnings
}
```

## What to read next

- `crates/ramshield-storage/` — stores the data this config sizes (RAM limits, shard counts)
- `crates/ramshield-detection/` — uses DetectionConfig thresholds
- `src/main.rs` — loads Config and passes it to the Engine
