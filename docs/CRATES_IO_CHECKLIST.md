# crates.io Publish Checklist — `ramshield`

*Backlog #47. [PENDING PUBLISH] — requires explicit user confirmation + `cargo publish`. No external action taken by agent.*

Backlog said `ramshield-core`; actual crate is **`ramshield`** (`Cargo.toml`).
Decide final published name before reserving on crates.io.

## Pre-publish gates

- [ ] `cargo publish --dry-run` passes
- [ ] `cargo package` produces clean tarball (check included files)
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test` green
- [ ] `README.md` renders on crates.io (relative links → absolute)

## Required `Cargo.toml` metadata (currently missing)

Add before publish:

```toml
[package]
description = "Rust-native DDoS mitigation engine: in-process flow tracking, rate limiting, anomaly scoring."
license = "MIT OR Apache-2.0"   # confirm vs LICENSE file (currently single LICENSE)
repository = "https://github.com/<org>/ramshield"
homepage = "https://github.com/<org>/ramshield"
readme = "README.md"
keywords = ["ddos", "security", "networking", "rate-limit", "firewall"]  # max 5
categories = ["network-programming"]
```

## Blockers

- `tflite-rust = "=0.3.0"` — verify it's published on crates.io; git/path deps block publish.
- Two `[[bin]]` + `[lib]` in one crate is fine to publish.
- Name collision: check `cargo search ramshield` before reserving.

## Publish (owner only — irreversible; a version can be yanked but never re-published)

1. `cargo login <token>`
2. `cargo publish --dry-run`
3. Review the packaged file list.
4. `cargo publish`

Note: publishing version 0.1.0 is permanent. You cannot delete or overwrite it — only yank. Bump the version to correct mistakes.
