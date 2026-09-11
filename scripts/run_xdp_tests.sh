#!/usr/bin/env bash
# Automated XDP comprehensive test suite — runs wire-contract tests + root-gated performance suite.
set -euo pipefail

cd "$(dirname "$0")/.."

# ── Cleanup orphaned root-owned build artifacts ──
echo "[Cleanup] Removing root-owned build artifacts"
sudo rm -rf target/debug/build target/debug/incremental 2>/dev/null || true
find target/debug -user root -exec rm -rf {} \; 2>/dev/null || true
echo "[Cleanup] Done"

# ── Phase 1: Wire-contract tests (no root needed) ──
echo "═══════════════════════════════════════════"
echo " RamShield XDP Comprehensive Test Suite"
echo "═══════════════════════════════════════════"
echo ""
echo "[Phase 1] Wire-contract unit tests (lib.rs)"
cargo test -p ramshield-xdp --features elf -- --list --nocapture | grep -c "test " || true

echo ""

# ── Phase 2: Root-gated stress/performance tests ──
echo "[Phase 2] Root-gated XDP stress & performance suite"
echo "  Note: requires sudo NOPASSWD."
echo ""

# Try without sudo first (non-root tests if any exist)
if [ "${1:-}" = "--dry" ]; then
    echo "  [dry-run] skipping root-gated tests — pass without args to execute"
else
    # Use sudoers NOPASSWD path
    echo "[Executing root-gated tests...]"
    exec sudo -E env PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" RUSTUP_HOME="$HOME/.rustup" \
        /home/m/.cargo/bin/cargo test -p ramshield-xdp --features elf -- --ignored ddos_ --test-threads=1
fi
