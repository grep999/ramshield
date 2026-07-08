#!/bin/bash
# Backup script for RamShield project
# Creates timestamped backups and keeps only the last 2

set -euo pipefail

PROJECT_DIR="/home/m/vehicle_of_rationalism/ramshield/beta/rs"
BACKUP_DIR="$PROJECT_DIR/backups"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
BACKUP_NAME="ramshield_backup_${TIMESTAMP}"
BACKUP_PATH="$BACKUP_DIR/${BACKUP_NAME}"

echo "Starting backup at $(date)"
echo "Project dir: $PROJECT_DIR"
echo "Backup dir: $BACKUP_DIR"

# Create backup directory if it doesn't exist
mkdir -p "$BACKUP_DIR"

# Create backup (exclude target directory and backups themselves)
tar -czf "${BACKUP_PATH}.tar.gz" \
    -C "$PROJECT_DIR" \
    --exclude="target" \
    --exclude="backups" \
    --exclude="*.log" \
    .

echo "Backup created: ${BACKUP_PATH}.tar.gz"

# Keep only the last 2 backups
echo "Cleaning up old backups (keeping last 2)..."
cd "$BACKUP_DIR"
ls -t ramshield_backup_*.tar.gz 2>/dev/null | tail -n +3 | xargs -r rm -f

echo "Remaining backups:"
ls -la "$BACKUP_DIR"/ramshield_backup_*.tar.gz 2>/dev/null || echo "No backups found"

echo "Backup completed at $(date)"