#!/usr/bin/env bash
# RamShield User-level (non-root) installer.
# WARNING: Does NOT provide kernel-level XDP/eBPF blocking.
# Only userspace proxy. Run as non-root; config/state in ~/.local.
set -eu
REPO="grep999/ramshield"
VERSION="${VERSION:-}"
PREFIX="${PREFIX:-$HOME/.local}"
CONFIG_DIR="$HOME/.config/ramshield"
STATE_DIR="$HOME/.local/share/ramshield"
COSIGN_BIN="${COSIGN_BIN:-cosign}"
LOG="/tmp/ramshield-user-install-$(date +%Y%m%d-%H%M%S).log"
log(){ printf '[ramshield-user-install] %s\n' "$*" | tee -a "$LOG"; }
die(){ printf '[ramshield-user-install] ERROR: %s\n' "$*" >&2 | tee -a "$LOG"; exit 1; }
need(){ command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }

exec > >(tee -a "$LOG") 2>&1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) [[ $# -ge 2 ]] || die '--version requires a value'; VERSION="$2"; shift 2;;
    --prefix) [[ $# -ge 2 ]] || die '--prefix requires a value'; PREFIX="$2"; shift 2;;
    -h|--help) echo "$0 --version X.Y.Z [--prefix PATH]"; exit 0;;
    *) die "unknown argument: $1";;
  esac
done
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'VERSION=x.y.z is required; latest/unpinned installs are forbidden'
for c in curl tar sha256sum install od tr sed; do need "$c"; done
need "$COSIGN_BIN"
case "$(uname -m)" in
  x86_64|amd64) TARGET=x86_64-unknown-linux-gnu;;
  aarch64|arm64) TARGET=aarch64-unknown-linux-gnu;;
  *) die "unsupported architecture: $(uname -m)";;
esac
BASE="https://github.com/${REPO}/releases/download/v${VERSION}"
ART="ramshield-${VERSION}-${TARGET}.tar.gz"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
for suffix in '' .sha256 .sig .pem; do
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$BASE/${ART}${suffix}" -o "$TMP/${ART}${suffix}"
done
(cd "$TMP" && sha256sum -c "${ART}.sha256") || die 'artifact checksum verification failed'
"$COSIGN_BIN" verify-blob --signature "$TMP/${ART}.sig" --certificate "$TMP/${ART}.pem" --certificate-identity-regexp '^https://github.com/grep999/ramshield/.github/workflows/release.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+$' --certificate-oidc-issuer https://token.actions.githubusercontent.com "$TMP/$ART" >/dev/null || die 'Sigstore artifact verification failed'
mkdir -p "$TMP/unpack"; tar -xzf "$TMP/$ART" -C "$TMP/unpack"
[[ -x "$TMP/unpack/ramshield" ]] || die 'invalid release archive'
[[ -x "$TMP/unpack/ramshield-cli" ]] || die 'release archive missing ramshield-cli'
"$TMP/unpack/ramshield" --version | grep -F "ramshield ${VERSION}" >/dev/null || die 'release binary version mismatch'
if [[ ! -d "$CONFIG_DIR" ]]; then
  mkdir -p "$CONFIG_DIR"
  chmod 0700 "$CONFIG_DIR"
fi
if [[ ! -f "$CONFIG_DIR/config.toml" ]]; then
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$BASE/config.baseline.toml" -o "$CONFIG_DIR/config.toml"
  IPC_KEY="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
  [[ "$IPC_KEY" =~ ^[0-9a-f]{64}$ ]] || die 'failed to generate IPC authentication key'
  sed -i "s/k1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/k1:$IPC_KEY/" "$CONFIG_DIR/config.toml"
  chmod 0600 "$CONFIG_DIR/config.toml"
fi
# Create state dir (non-root)
mkdir -p "$STATE_DIR/wal" "$STATE_DIR/snapshots"
# Install binaries to $PREFIX/bin (user-local)
mkdir -p "$PREFIX/bin"
install -m 0755 "$TMP/unpack/ramshield" "$PREFIX/bin/ramshield"
install -m 0755 "$TMP/unpack/ramshield-cli" "$PREFIX/bin/ramshield-cli"
log "installed v${VERSION} to $PREFIX/bin (userspace proxy only; no XDP/eBPF, no systemd)"
log "config: $CONFIG_DIR/config.toml"
log "state:  $STATE_DIR"
log "LIMITATIONS (non-root):"
log "  - No kernel XDP/eBPF blocking (userspace proxy only)"
log "  - No systemd service auto-start"
log "  - Cannot bind XDP interfaces without CAP_NET_ADMIN"
log "  - Rate limited by userspace, not kernel LruHashMap"
log "  - Manual process management required"
log "To use XDP/eBPF, re-run as root: sudo $0 --version ${VERSION}"
log "log file: $LOG"