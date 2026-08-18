#!/usr/bin/env bash
# scripts/check_guardrails.sh
# Automated guardrails for RamShield

set -euo pipefail

echo "🔍 Running code format check..."
cargo fmt -- --check

echo "🔍 Running clippy (strict)..."
cargo clippy -- -D warnings

echo "🔍 Running tests..."
cargo test -- --nocapture

echo "✅ All guardrails passed."