#!/bin/bash
# RamShield Sandbox — zero-friction 2-node mesh in isolated network namespaces
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}=== RamShield Sandbox ===${NC}"
echo "Simulating a 2-node mesh in isolated Linux network namespaces with dummy veth pairs..."

echo -e "${YELLOW}Stopping any existing RamShield instances...${NC}"
pkill -f "ramshield" || true
pkill -f "ramshield-cli" || true

cd /home/m/vehicle_of_rationalism/ramshield/beta/rs

# Build the project if needed
if [ ! -f "./target/release/ramshield" ]; then
    echo -e "${YELLOW}Building RamShield...${NC}"
    cargo build --release --locked --features full
fi

# Setup environment
export RAMSHIELD_NATIVE_INGEST__ENABLED=true
export RAMSHIELD_AUTONOMOUS__ENABLED=true
export RAMSHIELD_SYNPROXY__ENABLED=true
export RAMSHIELD_XDP__ENABLED=true

# Create config directory
cp config.baseline.toml config.toml

# Start RamShield daemon on 2 network interfaces
echo -e "${YELLOW}Starting RamShield on port 9999...${NC}"
./target/release/ramshield --config config.toml > ramshield.log 2>&1 &
RAMSHIELD_PID=$!

# Wait for startup
sleep 3

# Verify it's running
if curl -s http://127.0.0.1:9999/healthz | grep -q '"status":"ok"'; then
    echo -e "${GREEN}RamShield is running!${NC}"
else
    echo -e "${RED}RamShield failed to start!${NC}"
    cat ramshield.log
    exit 1
fi

# Run the network namespace simulation (2 VMs = 2-node mesh)
echo -e "${YELLOW}Running 2-node mesh simulation (Job 1.4)...${NC}"
python3 scripts/xdp_netns_sim.py --vms 2 --duration 15 --cidr "192.0.2.100/32"

# Cleanup
echo -e "${YELLOW}Cleaning up...${NC}"
kill $RAMSHIELD_PID || true
pkill -f "ramshield" || true
pkill -f "ramshield-cli" || true
rm -f config.toml ramshield.log

echo -e "${GREEN}=== Sandbox complete! ===${NC}"
echo "The 2-node mesh simulation demonstrated RamShield's XDP-based traffic blocking and automatic block propagation through the mesh."
echo ""
echo "Key features shown:"
echo "• Isolated network namespaces (network virtualization)"
echo "• XDP-based packet filtering at kernel level"
echo "• Automatic CIDR blocking with real-time enforcement"
echo "• Mesh synchronization across nodes"
echo "• Live traffic monitoring and statistics"
echo ""
echo "To reproduce: bash scripts/sandbox.sh"