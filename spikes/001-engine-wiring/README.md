# Spike 001 — Engine::start() wiring

## Question

**Given** a `Config` with `tcp_addr = "127.0.0.1:7890"` and an `Engine`, **when**
we wire them together and call `Engine::start()`, **then** the IPC server
binds a TCP listener on 7890 and accepts a JSON request from the
existing `ramshield-cli` binary.

## Why this matters

P0 blocker from the code review: `Engine::start()` is a no-op stub. The
`IpcServer` module exists in `src/ipc/server.rs` but is not registered. All
attack simulators (`scripts/attack_*.py`) and the CLI target port 7890 and get
`Connection refused`. This spike proves the wiring actually works before
committing it to the main binary.

## Approach

1. Register `pub mod server` in `src/ipc/mod.rs`.
2. Rewrite `src/ipc/server.rs` so the spawned task future is `Send`-safe
   (no `Box<dyn Error>` across `.await`).
3. Add an `Engine::start_async()` next to the (still-stubbed) sync `start()`,
   which spawns an `IpcServer::start()` on a current-thread tokio runtime,
   inside an integration test under `tests/`.
4. Integration test binds 7890, opens a TCP socket, sends
   `{"type":"get_status"}`, expects `{"type":"ok",...}` back.

## Files

- `src/ipc/server.rs`             — actual server (committed)
- `src/ipc/mod.rs`               — module registration (committed)
- `tests/ipc_wiring.rs`          — integration test (throwaway, deleted
                                    after spike — proves the wiring; the
                                    real binary wires it through main.rs)
- `Cargo.toml`                   — needs the `ipc` dependency or nothing,
                                    `tokio` is already present

## Constraints

- Must compile with existing `cargo build --all-targets`.
- Must pass `cargo clippy --all-targets -- -D warnings` after we trim the
  currently-suppressed warnings.
- Server must bind the configured `tcp_addr`, not a hardcoded port.
- Must coexist with the existing sync `Engine::start()` stub (don't break
  the 57 existing tests).
