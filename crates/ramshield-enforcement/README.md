# ramshield-enforcement

IP blocking enforcement with WAL-backed persistence and XDP integration.

## Architecture

```
EnforcementCommand (from Forecaster/Engine)
    │
    ▼
EnforcementEngine::apply()
    ├── write to Store (blocked_set + TTL)
    ├── write to WAL (for persistence across restarts)
    ├── WAL replay on startup (restore blocked IPs)
    └── optional: XDP program updates (block/allow lists)
```

## Key Components

### EnforcementEngine
- `apply(command)`: processes EnforceCommand (Block/Unblock)
  - Block: inserts into Store with TTL, writes WAL record
  - Unblock: removes from Store, writes WAL record
- WAL replay on startup: restores all non-expired blocks
- TTL expiry: lazy cleanup on access + periodic sweep

### WAL Integration
- Each enforcement action produces a WAL record
- On crash/restart: WAL replay restores blocked IPs
- Expired TTLs are skipped during replay
- Idempotent: duplicate replay is safe

## 2 tests
- WAL roundtrip: block → WAL write → restart → replay → block active
- Unblock cancels block: WAL records cancel out correctly
