# Workspace scope decision (A-021)

**Decision date:** 2026-10-10
**Branch:** `audit-branch`
**Status:** **RESOLVED - nested `rs/` is not a maintained or shipped product tree.**

## Decision

The **production RamShield product** is the **repository root workspace** only:

- Root `Cargo.toml` workspace members under `crates/*` and the root `ramshield` package
- Root CI workflows (`.github/workflows/ci.yml`, `Backlog Gate`, etc.)
- Root release/binary path: `cargo build --release --locked --features full` → `target/release/ramshield`

The nested tree at **`rs/`** is **not**:

- A second supported product
- Part of the root Cargo workspace
- Covered by root CI, Backlog Gate, or release packaging as a deployable surface
- A source of production security claims (including nested SHM writer behavior)

**Classification:** archival / experimental snapshot. Safe to keep in-tree for historical reference; **not release-ready**.

## Consequences

| Area | Rule |
|------|------|
| Production fixes | Apply only under the **root** tree (`src/`, `crates/`, root configs/scripts) |
| A-004 (SHM writer) | **Root only:** `crates/ramshield-cgnat/src/shm.rs` (and matching C header under root). Nested `rs/crates/ramshield-cgnat` is out of production scope |
| CI | Must not add a second full product matrix for `rs/` unless this decision is formally reversed |
| Docs / 100/100 claims | Must not treat `rs/` behavior as evidence of production readiness |
| Future revival | Requires explicit decision + pinned toolchain CI + lockfile + parity tests against root before any "maintained" status |

## Evidence (current tree)

- Root `Cargo.toml` `members` list does **not** include `rs/` or `rs/crates/*`
- Root workflows operate on the root workspace paths
- Nested `rs/` has its own `Cargo.toml` / `Cargo.lock` and diverges (e.g. SHM publish path still uses non-exclusive `fetch_add` seq transitions in places the root fixed with even→odd CAS)

## Reversal criteria

To reclassify `rs/` as maintained, all of the following must land on one SHA:

1. Written product owner approval
2. Independent CI workflow with pinned toolchain and `--locked` builds/tests for `rs/`
3. Documented parity policy vs root (or explicit "fork" versioning)
4. Security review of nested enforcement/SHM/replay paths
5. Update of this file and `BACKLOG.md` A-021 status

Until then, **`rs/` remains NOT APPLICABLE to production release gates.**
