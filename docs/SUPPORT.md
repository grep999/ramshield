# Support matrix

Best effort, no warranty.

| Item | Supported |
| :--- | :--- |
| Kernel | Linux 6.x with XDP-capable driver |
| Distros | Ubuntu 24.04, Debian 12 (clean-VM verification pending) |
| Arch | x86_64 tested; others best effort |
| Userspace | Rust stable/nightly per `Cargo.toml` |
| XDP modes | `drv`, `generic`; see `docs/XDP_DEVELOPMENT.md` |

Not supported: Windows, macOS XDP, kernel <5.x, L7 offload.
