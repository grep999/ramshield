#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(scripts/release_version.sh)"
TAG="${1:-$VERSION}"
[[ "$TAG" == "$VERSION" ]] || { echo "tag $TAG != Cargo.toml $VERSION" >&2; exit 1; }
XDP_ENABLED=false
[[ "${2:-}" == --no-xdp ]] && XDP_ENABLED=false
[[ -x target/release/ramshield ]] || { echo 'build target/release/ramshield first' >&2; exit 1; }
CTX="$(mktemp -d)"; trap 'rm -rf "$CTX"' EXIT
cp target/release/ramshield config.baseline.toml LICENSE docker/Dockerfile "$CTX/"
cat > "$CTX/.dockerignore" <<'EOF'
target/
*.log
config.prod.toml
EOF
docker build --build-arg RAMSHIELD_VERSION="$VERSION" --build-arg XDP_ENABLED="$XDP_ENABLED" -t "ghcr.io/grep999/ramshield:$TAG" "$CTX"
if [[ "${PUSH:-0}" == 1 ]]; then docker push "ghcr.io/grep999/ramshield:$TAG"; fi
