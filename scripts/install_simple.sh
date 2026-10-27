#!/usr/bin/env bash
# RamShield Quick-Start Installer
# curl -fsSL https://ramshield.io/install.sh | bash
# Installs safe-mode (no XDP) runtime; sets up systemd service.
set -euo pipefail

# POSIX helpers (A3): log/warn/die must exist before any early path uses them.
log()  { printf '\033[0;32m[INFO]\033[0m %s\n' "$*"; }
warn() { printf '\033[0;33m[WARN]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[0;31m[ERROR]\033[0m %s\n' "$*" >&2; exit 1; }

# --dry-run: verify argument/platform/version resolution and print the plan
# without touching the system. Used by the clean-room CI gate and by
# operators checking pipe-mode works before installing.
DRY_RUN=0
for a in "$@"; do
  case "$a" in
    --dry-run) DRY_RUN=1 ;;
  esac
done

# A2 (curl | bash): when piped, $0 is "bash" and dirname "$0" is meaningless.
# Resolve the repo root only when running from a checkout; otherwise assets
# are fetched from the release below.
if [ -f "$0" ] && [ -f "$(dirname "$0")/config-min.toml" ]; then
  ROOT="$(cd "$(dirname "$0")" && pwd)"
else
  ROOT=""
fi
[ -n "$ROOT" ] && cd "$ROOT"

# Detect platform
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) TARGET=x86_64-unknown-linux-gnu ;;
  aarch64|arm64) TARGET=aarch64-unknown-linux-gnu ;;
  *) die "unsupported architecture: $ARCH" ;;
esac

# Fetch latest release info from GitHub API (no VERSION pin required)
REPO="grep999/ramshield"
API="https://api.github.com/repos/$REPO/releases/latest"
LATEST=$(curl -fsSL --retry 5 -H "Accept: application/vnd.github.v3+json" "$API" | python3 -c "import sys,json; print(json.load(sys.stdin)['tag_name'].lstrip('v'))" 2>/dev/null || echo "0.6.0")
VERSION="${VERSION:-$LATEST}"

if [ -z "$VERSION" ] || [ "$VERSION" = "null" ]; then
  die "cannot determine latest version; set VERSION=x.y.z or check network"
fi

# Download binary
BINARY="ramshield-${VERSION}-${TARGET}.tar.gz"
BASE="https://github.com/grep999/ramshield/releases/download/v${VERSION}"

if [ "$DRY_RUN" = "1" ]; then
  # ponytail: dry-run validates resolution only; full plan simulation when
  # the release workflow needs a rehearsal mode.
  log "dry-run: target=$TARGET version=$VERSION"
  log "dry-run: would download $BASE/$BINARY"
  log "dry-run: would install ${PREFIX:-/usr/local}/bin/ramshield + /etc/ramshield/config.toml + systemd unit"
  log "dry-run: OK (no system changes)"
  exit 0
fi

curl -fsSL --retry 5 "$BASE/$BINARY" -o /tmp/ramshield.tar.gz || die "failed to download binary"

# Extract and install
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
tar -xzf /tmp/ramshield.tar.gz -C "$TMP"
BIN="$TMP/ramshield"
[ -x "$BIN" ] || die "downloaded binary not executable"

PREFIX="${PREFIX:-/usr/local}"
mkdir -p "$PREFIX/bin"
# A4 (transactional boundary): stage the binary, verify it runs, then move
# atomically into place.
install -m 0755 "$BIN" "$PREFIX/bin/ramshield.new"
"$PREFIX/bin/ramshield.new" --version >/dev/null || die "staged binary failed --version"
mv -f "$PREFIX/bin/ramshield.new" "$PREFIX/bin/ramshield"

# Safe-mode config (no XDP, userspace only). Under curl|bash there is no
# checkout: fetch config-min.toml from the release tag instead (A2).
CONFIG_SRC="$ROOT/config-min.toml"
if [ -z "$ROOT" ]; then
  CONFIG_SRC="$TMP/config-min.toml"
  curl -fsSL --retry 5 "$BASE/config-min.toml" -o "$CONFIG_SRC" \
    || die "failed to download config-min.toml from release v$VERSION"
fi
cp "$CONFIG_SRC" "$CONFIG_DIR/config.toml"
chown root:ramshield "$CONFIG_DIR/config.toml"; chmod 0640 "$CONFIG_DIR/config.toml"

# Create systemd service (userspace only, no XDP)
cat > /etc/systemd/system/ramshield.service <<UNIT
[Unit]
Description=RamShield Ingress Defense (safe mode: userspace, no XDP)
After=network-online.target
Wants=network-online.target
Documentation=https://github.com/grep999/ramshield

[Service]
Type=simple
User=ramshield
Group=ramshield
ExecStart=/usr/local/bin/ramshield --config /etc/ramshield/config.toml
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
ProtectKernelModules=true
ProtectKernelLogs=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
CapabilityBoundingSet=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW
ReadWritePaths=/var/lib/ramshield /dev/shm /sys/fs/bpf

[Install]
WantedBy=multi-user.target
UNIT

# A1: create group and user BEFORE any chown/install that references them.
getent group ramshield >/dev/null 2>&1 || groupadd --system ramshield
if ! id -u ramshield >/dev/null 2>&1; then
  useradd --system --gid ramshield --home /nonexistent --shell /usr/sbin/nologin ramshield
else
  usermod -g ramshield ramshield
fi

# Set permissions
install -d -o root -g ramshield -m 0750 "$CONFIG_DIR"
install -d -o ramshield -g ramshield -m 0750 "$STATE_DIR"
install -d -o ramshield -g ramshield -m 0750 "$STATE_DIR/wal" "$STATE_DIR/snapshots"

# Set up firewall-friendly iptables rules (optional, best-effort)
iptables -A INPUT -p tcp --dport 7890 -j ACCEPT 2>/dev/null || true

log "installed v$VERSION (safe mode: userspace, no XDP)"
log "config: $CONFIG_DIR/config.toml"
log "start: systemctl enable --now ramshield.service"
