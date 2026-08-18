# RamShield `unwrap()` audit

Source: attached grep of `src/**/*.rs` (tests not excluded by the grep; `unwrap_or` and non-`.unwrap()` context lines dropped).

**87 real `.unwrap()` calls** in the attached listing (claimed 93 — 6 extra were `state.write().await` / `String::new()` / `thread_rng()` context, plus 2 `unwrap_or`).

Lock poison (`Mutex`/`RwLock`) outranks file-path buckets.

## Summary

| Priority | Count | Meaning |
|----------|------:|---------|
| **P0 Critical** | 39 | `Mutex`/`RwLock` `.lock()`/`.read()`/`.write()` — poison panics the thread |
| **P1 High** | 2 | `Option`/`Result` on engine request/runtime path |
| **P2 Medium** | 46 | config/init, dashboard HTTP, WAL I/O, `Engine::new` |
| **P3 Low** | 0 | none after dropping `unwrap_or` / test-helper noise |
| **Total** | **87** | |

### Suggested fix order

1. **P0** — `unwrap_or_else(|e| e.into_inner())` or `parking_lot` (no poison). Hot path first: `detection/`, `storage/ttl_wheel.rs`, `storage/wal.rs`, `storage/blob_store.rs`, `metrics/`.
2. **P1** — `Engine` tokio runtime: `expect("tokio runtime")` at boot is fine; do not leave bare `unwrap`.
3. **P2** — `?` + `thiserror`/`anyhow`. Dashboard JSON must not panic on bad admin input. WAL I/O already has `Result` on some paths — stop unwrapping it.
4. Dashboard `mod.rs` is **6 copy-pasted handler blocks** (same 4 unwraps × 6). Dedup before fixing.

---

## P0 — lock poison (39)

Replace with `unwrap_or_else(PoisonError::into_inner)` or `parking_lot::{Mutex,RwLock}`.

### `src/alerting/mod.rs` (3) — Mutex, alert cooldown/history

| Line | What | Code |
|-----:|------|------|
| 162 | Mutex lock `last_alert_ms` | `let mut last = self.last_alert_ms.lock().unwrap();` |
| 198 | Mutex lock `alert_history` | `let mut history = self.alert_history.lock().unwrap();` |
| 240 | Mutex lock `alert_history` | `self.alert_history.lock().unwrap().clone()` |

### `src/cache.rs` (2) — RwLock write on store

| Line | What | Code |
|-----:|------|------|
| 107 | RwLock write `store` | `let mut store = self.store.write().unwrap();` |
| 136 | RwLock write `store` | `let mut store = self.store.write().unwrap();` |

### `src/dashboard/mod.rs` (6) — RwLock write on live config (HTTP path)

| Line | What | Code |
|-----:|------|------|
| 121 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |
| 255 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |
| 411 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |
| 567 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |
| 723 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |
| 879 | RwLock write `state.config` | `let mut config_guard = state.config.write().unwrap();` |

### `src/detection/batch.rs` (2) — Mutex, **hot path** buffer

| Line | What | Code |
|-----:|------|------|
| 50 | Mutex lock `buffer` | `let mut buffer = self.buffer.lock().unwrap();` |
| 141 | Mutex lock `buffer` | `let mut buf = self.buffer.lock().unwrap();` |

### `src/detection/mod.rs` (2) — RwLock, **hot path** detector state

| Line | What | Code |
|-----:|------|------|
| 347 | RwLock write `state` | `let mut state = self.state.write().unwrap();` |
| 408 | RwLock write `state` | `let mut state = self.state.write().unwrap();` |

### `src/dns/mod.rs` (1) — RwLock write on DNS cache

| Line | What | Code |
|-----:|------|------|
| 136 | RwLock write `cache` | `let mut cache = self.cache.write().unwrap();` |

### `src/learning/mod.rs` (3) — RwLock on pattern map

| Line | What | Code |
|-----:|------|------|
| 98 | RwLock read `patterns` | `let patterns = self.patterns.read().unwrap();` |
| 137 | RwLock write `patterns` | `let mut patterns = self.patterns.write().unwrap();` |
| 188 | RwLock read `patterns` | `let patterns = self.patterns.read().unwrap();` |

### `src/metrics/mod.rs` (2) — Mutex on snapshot

| Line | What | Code |
|-----:|------|------|
| 253 | Mutex lock `snapshot` | `let snapshot = self.snapshot.lock().unwrap().clone();` |
| 260 | Mutex lock `snapshot` | `let mut snapshot = self.snapshot.lock().unwrap();` |

### `src/storage/blob_store.rs` (5) — Mutex on blob DB

| Line | What | Code |
|-----:|------|------|
| 75 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |
| 85 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |
| 95 | Mutex lock `db` | `let mut db = self.db.lock().unwrap();` |
| 114 | Mutex lock `db` | `let mut db = self.db.lock().unwrap();` |
| 125 | Mutex lock `db` | `let mut db = self.db.lock().unwrap();` |

### `src/storage/mod.rs` (2) — RwLock write on store state

| Line | What | Code |
|-----:|------|------|
| 82 | RwLock write `state` | `let mut state = self.state.write().unwrap();` |
| 373 | RwLock write `state` | `let mut state = self.state.write().unwrap();` |

### `src/storage/ttl_wheel.rs` (6) — Mutex on wheel slots (expiry path)

| Line | What | Code |
|-----:|------|------|
| 69 | Mutex lock `wheel` | `let mut wheel = self.wheel.lock().unwrap();` |
| 86 | Mutex lock `wheel` | `let mut wheel = self.wheel.lock().unwrap();` |
| 96 | Mutex lock `wheel` | `let wheel = self.wheel.lock().unwrap();` |
| 109 | Mutex lock `wheel` | `let mut wheel = self.wheel.lock().unwrap();` |
| 132 | Mutex lock `wheel` | `let mut wheel = self.wheel.lock().unwrap();` |
| 141 | Mutex lock `wheel` | `let mut wheel = self.wheel.lock().unwrap();` |

### `src/storage/wal.rs` (5) — Mutex on WAL/db

| Line | What | Code |
|-----:|------|------|
| 70 | Mutex lock `db` | `let mut db = self.db.lock().unwrap();` |
| 113 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |
| 140 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |
| 168 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |
| 197 | Mutex lock `db` | `let db = self.db.lock().unwrap();` |

---

## P1 — engine / request path Result (2)

| File:line | What | Code |
|-----------|------|------|
| `src/engine/mod.rs:191` | tokio current-thread runtime `build()` | `let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();` |
| `src/engine/mod.rs:276` | same | `let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();` |

`build()` fails only on IO/thread limits. Boot-time `expect` is acceptable; do not panic later on the snapshot/IPC path.

No `ipc/server.rs` `.unwrap()` in the attached grep.

---

## P2 — config, dashboard, init, WAL I/O (46)

### `src/config.rs` (6) — file read + TOML parse

| Line | What | Code |
|-----:|------|------|
| 169 | `Read::read_to_string` | `file.read_to_string(&mut contents).unwrap();` |
| 170 | `toml::from_str` | `toml::from_str(&contents).unwrap()` |
| 220 | `Read::read_to_string` | `file.read_to_string(&mut contents).unwrap();` |
| 221 | `toml::from_str` | `toml::from_str(&contents).unwrap()` |
| 226 | `Read::read_to_string` | `file.read_to_string(&mut contents).unwrap();` |
| 227 | `toml::from_str` | `toml::from_str(&contents).unwrap()` |

Use existing `Config::from_toml_file` (`?`). These look like duplicated loaders (or tests without `#[cfg(test)]` exclusion).

### `src/main.rs` (1) — process init

| Line | What | Code |
|-----:|------|------|
| 85 | `Engine::new` Result | `let engine = Engine::new(config.clone()).unwrap();` |

`main` can `?` via `anyhow`.

### `src/dashboard/mod.rs` (18) — HTTP config patch + TOML reload (×6 clones)

Each clone: JSON patch unwrap, file read unwrap, TOML unwrap.

| Lines | What | Code |
|------:|------|------|
| 119, 253, 409, 565, 721, 877 | `serde_json::from_value` (admin body) | `let patch: ConfigPatch = serde_json::from_value(json).unwrap();` |
| 197, 331, 487, 643, 799, 955 | `fs::read_to_string` | `let contents = std::fs::read_to_string(path).unwrap();` |
| 198, 332, 488, 644, 800, 956 | `toml::from_str` | `toml::from_str(&contents).unwrap()` |

Bad JSON currently 500-panics the worker. Return 400.

### `src/storage/wal.rs` (21) — durability I/O (not lock)

Panic here drops WAL records. Prefer `?` (fn already returns `Result` in current tree).

| Lines | What | Pattern |
|------:|------|---------|
| 59, 77, 94, 125, 153, 182, 210 | `OpenOptions::open` | `OpenOptions::new().create(true).append(true).open(path).unwrap()` |
| 60, 80, 97, 128, 156, 185, 213 | `serde_json::to_writer` | `serde_json::to_writer(&file, &entry).unwrap()` |
| 61, 81, 98, 129, 157, 186, 214 | `Write::write_all` newline | `file.write_all(b"\n").unwrap()` |

---

## P3 — excluded from attached grep

| Line | Why dropped |
|------|-------------|
| `src/util/mod.rs:105` | `unwrap_or(0)`, not `unwrap()` |
| `src/util/mod.rs:156` | `unwrap_or(0)`, not `unwrap()` |
| `src/config.rs:168,219,225` | `String::new()` context |
| `src/dashboard/mod.rs:118,252,408,564,720,876` | `state.write().await` (tokio, not poison) |
| `src/util/mod.rs:103` | `thread_rng()` context |

---

## Counts by file (attached grep)

| File | P0 | P1 | P2 | Total |
|------|---:|---:|---:|------:|
| `src/storage/wal.rs` | 5 | 0 | 21 | 26 |
| `src/dashboard/mod.rs` | 6 | 0 | 18 | 24 |
| `src/storage/ttl_wheel.rs` | 6 | 0 | 0 | 6 |
| `src/config.rs` | 0 | 0 | 6 | 6 |
| `src/storage/blob_store.rs` | 5 | 0 | 0 | 5 |
| `src/alerting/mod.rs` | 3 | 0 | 0 | 3 |
| `src/learning/mod.rs` | 3 | 0 | 0 | 3 |
| `src/cache.rs` | 2 | 0 | 0 | 2 |
| `src/detection/batch.rs` | 2 | 0 | 0 | 2 |
| `src/detection/mod.rs` | 2 | 0 | 0 | 2 |
| `src/metrics/mod.rs` | 2 | 0 | 0 | 2 |
| `src/storage/mod.rs` | 2 | 0 | 0 | 2 |
| `src/engine/mod.rs` | 0 | 2 | 0 | 2 |
| `src/dns/mod.rs` | 1 | 0 | 0 | 1 |
| `src/main.rs` | 0 | 0 | 1 | 1 |
| **Sum** | **39** | **2** | **46** | **87** |

---

## Live tree note (2026-08-18)

Attached grep is **stale**. Current `rs/` has already removed most config/dashboard/WAL I/O unwraps (`?` in `Config::from_toml_file`, WAL `append`). New production `.unwrap()` (excluding `#[cfg(test)]` / benches):

**Still P0 (locks):** `alerting` 3, `detection/mod` bloom 2, `engine` metrics history 2, `learning` + `crates/ramshield-learning` 3+3, `metrics` + `crates/ramshield-metrics` SYS mutex 1+1, `storage/{blob_store,wal,ttl_wheel}` + crate copies.

**New non-lock production:**

| File:line | Pri | What |
|-----------|-----|------|
| `src/main.rs:59` | P2 | `Path::to_str().unwrap()` after `canonicalize` |
| `src/metrics/mod.rs:28` / crate twin | P2 | `Option` `guard.as_mut().unwrap()` after fill |
| `src/storage/wal.rs:133,137,138` | P2 | `[u8]::try_into().unwrap()` after length check (infallible) |
| `crates/ramshield-storage/src/wal.rs:75-79` | P2 | same, `RecordHeader::from_bytes` |
| `crates/ramshield-protocol/src/codec.rs:134` | P1 | CRC `try_into().unwrap()` on IPC decode path |
| `crates/ramshield-xdp/src/lib.rs:86,95,119` | P2 | `program_mut` / `take_map` / `bpf.as_mut()` at XDP load |

Refactor against **live** sites, not the attached line numbers. Dashboard/config/WAL I/O rows above are historical.
