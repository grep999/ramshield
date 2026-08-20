# Enforcement Connectivity Fix Plan

## Current State (BROKEN)
- DetectionEngine uses `broadcast::Sender<BlockDecision>` 
- Forecaster uses `broadcast::Sender<BlockDecision>`
- **NO EnforcementService is started**
- Block decisions are emitted but never consumed/applied

## Required Changes

### 1. src/detection/mod.rs
- Replace `broadcast::Sender<BlockDecision>` with `mpsc::Sender<EnforceCommand>`
- Change `BlockDecision` structs to `EnforceCommand { action: EnforceAction::Block }`
- Update `flush_batch()` to send via `enforce_tx.send(cmd).await`
- Update `subnet_batch_loop()` similarly

### 2. crates/ramshield-forecasting/src/lib.rs  
- Replace broadcast receiver with mpsc command sender
- Send `EnforceCommand` instead of `BlockDecision`

### 3. src/engine/mod.rs (boot_pipeline)
```rust
let (cmd_tx, cmd_rx) = mpsc::channel::<EnforceCommand>(1024);
let shutdown_tx = watch::channel(false);

// Start EnforcementService
let enforcement = EnforcementService::new(
    store.clone(),
    metrics.clone(),
    Box::new(StubXdpApplier),
    shutdown_rx,
);
tokio::spawn(async move { enforcement.run(cmd_rx).await });

// Pass cmd_tx to DetectionEngine and Forecaster
```

### 4. Crates that depend on BlockDecision
- Remove unused `BlockDecision` struct or keep for telemetry only
- Ensure all block emission paths use EnforceCommand

## Verification
After fix:
- `cargo build --all-targets` should pass
- Block decisions → EnforcementService → Store mutation → XDP apply
- Metrics.blocks_* should correlate with actual store blocks

## Priority
P0 - Security critical. Without this, blocks are logged but never enforced.
