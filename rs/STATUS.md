# Nested `rs/` tree — NOT a shipped product

**A-021 decision (2026-10-10):** this directory is an **archival / experimental** snapshot.

- **Do not** deploy binaries built only from `rs/`
- **Do not** treat tests or SHM/replay behavior here as production evidence
- **Do not** expect root CI to validate this tree
- Production work happens at the **repository root** only

See [`docs/WORKSPACE_SCOPE.md`](../docs/WORKSPACE_SCOPE.md).
