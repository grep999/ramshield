# ramshield-config

Configuration types and validation for all RamShield components.

## Config struct hierarchy

```
Config
├── detection: DetectionConfig
├── forecasting: ForecastingConfig  
├── ipc: IpcConfig (auth_keys, batch limits)
├── xdp: XdpConfig (enabled, interface, mode)
└── dashboard: DashboardConfig
```

## Validation rules
- `ram_limit_mb >= 128`
- `shard_count` must be power of 2
- `ipc.max_connections > 0`
- `ipc.auth_keys` must be present for non-loopback binds
- Public IPC/dashboard binds require auth_keys
