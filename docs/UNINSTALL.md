# Uninstall

```bash
sudo systemctl stop ramshield 2>/dev/null || true
sudo systemctl disable ramshield 2>/dev/null || true
sudo rm -f /etc/systemd/system/ramshield.service
sudo systemctl daemon-reload
sudo rm -f /usr/local/bin/ramshield
```

Config, WAL and snapshots under your chosen data dir are left in place. Delete them only after a verified backup.
