#!/usr/bin/env bash
# Canonical enterprise Linux installer. Explicit VERSION is mandatory.
set -euo pipefail
umask 027
REPO="grep999/ramshield"
VERSION="${VERSION:-}"
PREFIX="${PREFIX:-/usr/local}"
CONFIG_DIR=/etc/ramshield
STATE_DIR=/var/lib/ramshield
USER_NAME=ramshield
GROUP_NAME=ramshield
COSIGN_BIN="${COSIGN_BIN:-cosign}"
log(){ printf '[ramshield-install] %s\n' "$*"; }
die(){ printf '[ramshield-install] ERROR: %s\n' "$*" >&2; exit 1; }
need(){ command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) [[ $# -ge 2 ]] || die '--version requires a value'; VERSION="$2"; shift 2;;
    --prefix) [[ $# -ge 2 ]] || die '--prefix requires a value'; PREFIX="$2"; shift 2;;
    -h|--help) echo "$0 --version X.Y.Z [--prefix PATH]"; exit 0;;
    *) die "unknown argument: $1";;
  esac
done
[[ $EUID -eq 0 ]] || die 'run as root'
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'VERSION=x.y.z is required; latest/unpinned installs are forbidden'
for c in curl tar sha256sum install systemctl useradd getent groupadd usermod od tr sed; do need "$c"; done
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
"$COSIGN_BIN" verify-blob   --signature "$TMP/${ART}.sig"   --certificate "$TMP/${ART}.pem"   --certificate-identity-regexp '^https://github.com/grep999/ramshield/.github/workflows/release.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+$'   --certificate-oidc-issuer https://token.actions.githubusercontent.com   "$TMP/$ART" >/dev/null || die 'Sigstore artifact verification failed'
mkdir -p "$TMP/unpack"; tar -xzf "$TMP/$ART" -C "$TMP/unpack"
[[ -x "$TMP/unpack/ramshield" ]] || die 'invalid release archive'
[[ -x "$TMP/unpack/ramshield-cli" ]] || die 'release archive missing ramshield-cli'
"$TMP/unpack/ramshield" --version | grep -F "ramshield ${VERSION}" >/dev/null || die 'release binary version mismatch'
if ! id -u "$USER_NAME" >/dev/null 2>&1; then useradd --system --home /nonexistent --shell /usr/sbin/nologin "$USER_NAME"; fi
getent group "$GROUP_NAME" >/dev/null 2>&1 || groupadd --system "$GROUP_NAME"
usermod -g "$GROUP_NAME" "$USER_NAME"
install -d -o root -g "$GROUP_NAME" -m 0750 "$CONFIG_DIR"
install -d -o "$USER_NAME" -g "$GROUP_NAME" -m 0750 "$STATE_DIR" "$STATE_DIR/wal" "$STATE_DIR/snapshots"
install -o root -g root -m 0755 "$TMP/unpack/ramshield" "$PREFIX/bin/ramshield"
install -o root -g root -m 0755 "$TMP/unpack/ramshield-cli" "$PREFIX/bin/ramshield-cli"
if [[ ! -f "$CONFIG_DIR/config.toml" ]]; then
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$BASE/config.baseline.toml" -o "$CONFIG_DIR/config.toml"
  # Replace the tracked development credential with a unique 256-bit key.
  # The installed config remains HMAC-authenticated even on loopback.
  IPC_KEY="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
  [[ "$IPC_KEY" =~ ^[0-9a-f]{64}$ ]] || die 'failed to generate IPC authentication key'
  sed -i "s/k1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/k1:$IPC_KEY/" "$CONFIG_DIR/config.toml"
  chown root:"$GROUP_NAME" "$CONFIG_DIR/config.toml"; chmod 0640 "$CONFIG_DIR/config.toml"
fi
cat > /etc/systemd/system/ramshield.service <<UNIT
[Unit]
Description=RamShield Autonomous Ingress Defense
Documentation=https://github.com/${REPO}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${USER_NAME}
Group=${GROUP_NAME}
ExecStart=${PREFIX}/bin/ramshield --config ${CONFIG_DIR}/config.toml
Restart=on-failure
RestartSec=5s
StartLimitIntervalSec=60
StartLimitBurst=5
LimitNOFILE=65535
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ProtectControlGroups=true
ProtectKernelTunables=true
# The SYNPROXY tunables are managed by sysctl.d at boot; the daemon only
# reads them, so /proc/sys stays read-only.
ProtectKernelModules=true
ProtectKernelLogs=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
CapabilityBoundingSet=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW
ReadWritePaths=${STATE_DIR} /dev/shm /sys/fs/bpf

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
"$PREFIX/bin/ramshield" --version | grep -F "ramshield ${VERSION}" >/dev/null || die 'installed binary version mismatch'
systemctl enable ramshield.service >/dev/null
log "installed v${VERSION}; inspect ${CONFIG_DIR}/config.toml before starting"
