# Deployment models

## systemd
Install binary, config, unit; verify with `systemctl status`.

## Docker / Containerfile
Available in-tree; XDP attach needs host privileges and correct driver.

## Bare metal
One binary per node; WAL on local persistent disk.

No cloud account, no control plane, no mesh requirement for one-host defense.
