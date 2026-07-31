# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-07-31

### Added
- Created `AGENTS.md` to establish and enforce coding standards and project structure.
- Implemented a "Self-Healing Protocol" requiring `build`, `clippy`, and `test` to pass before completing tasks.
- Added `CHANGELOG.md` to track notable changes between versions.

### Changed
- Updated project guidelines based on analysis of mature Rust projects like `jcode`.
- Refined the build process and verification steps for better CI/CD practices.

### Fixed
- Resolved build failure by removing a dead import (`ConnectionEventRecord`) from `src/engine/mod.rs`.
- Fixed clippy error by removing an unused constant (`CONNECTION_EVENT_HISTORY`) from `src/metrics/mod.rs`.
- Corrected working directory issue in agent's verification script execution.
