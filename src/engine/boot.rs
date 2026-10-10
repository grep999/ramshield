use super::*;

/// Capability preflight for the XDP path.
///
/// BPF map creation fails with EPERM (surfaced by aya as "failed to create
/// map") when the process lacks CAP_BPF/CAP_NET_ADMIN. File capabilities live
/// on the inode, so every `cargo build` silently drops them — the failure then
/// looks like a map bug. Read CapEff and hand back the exact setcap command.
#[cfg(feature = "xdp")]
fn xdp_capability_hint() -> String {
    const CAP_NET_ADMIN: u64 = 12;
    const CAP_PERFMON: u64 = 38;
    const CAP_BPF: u64 = 39;
    let needed = [
        ("cap_net_admin", CAP_NET_ADMIN),
        ("cap_perfmon", CAP_PERFMON),
        ("cap_bpf", CAP_BPF),
    ];
    let eff = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("CapEff:"))
                .and_then(|l| u64::from_str_radix(l.split_whitespace().nth(1)?, 16).ok())
        })
        .unwrap_or(0);
    let missing: Vec<&str> = needed
        .iter()
        .filter(|(_, bit)| eff & (1u64 << bit) == 0)
        .map(|(name, _)| *name)
        .collect();
    if missing.is_empty() {
        return "capabilities present; check kernel BPF limits".to_string();
    }
    format!(
        "missing {} — re-apply after every build: sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf+eip' target/release/ramshield",
        missing.join(",")
    )
}

pub(crate) async fn boot_pipeline(engine: Arc<Engine>) -> std::io::Result<()> {
    // FIX: use engine.config directly for live hot-reload — not a separate ArcSwap.
    // The old code did cfg_snapshot.clone().into_handle() which created a parallel
    // ArcSwap that never saw updates from api_set_config.
    let cfg_handle = engine.config.clone();

    // Boot-time snapshot: read-once values (XDP, WAL, forecaster config).
    // These are immutable once the pipeline starts; changing them requires restart.
    let cfg_arc = engine.config.load(); // Arc<Config>
    let cfg_snapshot = cfg_arc.as_ref().clone(); // owned Config clone

    // Use engine's shared store and metrics (shared with dashboard)
    let store = engine.store.clone();
    let metrics = engine.metrics.clone();

    // Take the enforcement receiver ONCE
    let enforcement_rx = engine
        .enforcement_rx
        .lock()
        .map_err(|_| std::io::Error::other("enforcement receiver lock poisoned"))?
        .take()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "enforcement service already started",
            )
        })?;
    // The service follows the engine shutdown flag through a dedicated watcher.
    let enforcement_shutdown = Arc::new(AtomicBool::new(false));
    // Dataplane: real aya XDP when [xdp].enabled, else in-band-only stub.
    // When attach fails and allow_inband_fallback is false the pipeline fails
    // hard (operators must fix XDP or set allow_inband_fallback=true).
    let hard_xdp = cfg_snapshot.xdp.enabled && !cfg_snapshot.xdp.allow_inband_fallback;
    let xdp_box: Box<dyn XdpApplier> = if cfg_snapshot.xdp.enabled {
        #[cfg(feature = "xdp")]
        {
            let mut applier = crate::enforcement::xdp::AyaXdpApplier::new(
                &cfg_snapshot.xdp.interface,
                &cfg_snapshot.xdp.mode,
            );
            match applier.load_and_attach() {
                Ok(()) => {
                    if cfg_snapshot.autonomous.enabled
                        && let Err(e) = crate::engine::rss::rebalance(
                            &cfg_snapshot.xdp.interface,
                            cfg_snapshot.xdp.require_rss_rebalance,
                        )
                    {
                        if hard_xdp {
                            return Err(std::io::Error::other(format!(
                                "RSS rebalance required but unavailable: {e}"
                            )));
                        }
                        tracing::warn!(error = %e, "RSS rebalance unavailable");
                    }
                    if let Err(e) =
                        applier.configure_trusted_overlay(&cfg_snapshot.xdp.trusted_overlay_cidrs)
                    {
                        tracing::error!(error = %e, "failed to configure trusted overlay source prefixes");
                        if hard_xdp {
                            return Err(std::io::Error::other(format!(
                                "trusted overlay configuration failed: {e}"
                            )));
                        }
                    }
                    if let Err(e) = applier.configure_autonomous(
                        cfg_snapshot.autonomous.enabled,
                        cfg_snapshot.autonomous.syn_pps_per_cpu,
                        cfg_snapshot.autonomous.udp_pps_per_cpu,
                        cfg_snapshot.autonomous.packet_pps_per_cpu,
                        cfg_snapshot.autonomous.window_ms,
                    ) {
                        tracing::error!(error = %e, "failed to configure autonomous XDP guard");
                        if hard_xdp {
                            return Err(std::io::Error::other(format!(
                                "autonomous XDP configuration failed: {e}"
                            )));
                        }
                    }
                    tracing::info!(iface = %cfg_snapshot.xdp.interface, mode = %cfg_snapshot.xdp.mode, autonomous = cfg_snapshot.autonomous.enabled, "XDP dataplane active");
                    engine.xdp_active.store(true, Ordering::Release);
                    metrics.set_xdp_projection_active(true);
                    Box::new(applier)
                }
                Err(e) => {
                    // A rebuild replaces the binary inode and drops its file
                    // capabilities, which the kernel reports as an opaque map
                    // creation failure. Name the real cause instead.
                    tracing::error!(
                        iface = %cfg_snapshot.xdp.interface,
                        mode = %cfg_snapshot.xdp.mode,
                        error = %e,
                        remediation = %xdp_capability_hint(),
                        "XDP load/attach failed — falling back to in-band enforcement"
                    );
                    metrics.set_xdp_projection_active(false);
                    if hard_xdp {
                        return Err(std::io::Error::other(
                            "XDP configured and allow_inband_fallback=false — attach failed",
                        ));
                    }
                    Box::new(StubXdpApplier)
                }
            }
        }
        #[cfg(not(feature = "xdp"))]
        {
            tracing::error!("[xdp].enabled=true but binary built without 'xdp' feature");
            if hard_xdp {
                return Err(std::io::Error::other(
                    "XDP configured and allow_inband_fallback=false — binary has no xdp feature",
                ));
            }
            Box::new(StubXdpApplier)
        }
    } else {
        Box::new(StubXdpApplier)
    };
    #[cfg(feature = "mesh")]
    let mesh_blocklist = if cfg_snapshot.mesh.enabled {
        Some(Arc::new(AworsetBlocklist::new(cfg_snapshot.mesh.node_id)))
    } else {
        None
    };
    #[cfg(feature = "mesh")]
    let mesh_handle = if cfg_snapshot.mesh.enabled {
        let listen = cfg_snapshot.mesh.listen_addr.parse().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid mesh.listen_addr: {e}"),
            )
        })?;
        let mut peers = Vec::with_capacity(cfg_snapshot.mesh.peers.len());
        for peer in &cfg_snapshot.mesh.peers {
            peers.push(peer.parse().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid mesh peer {peer}: {e}"),
                )
            })?);
        }
        let key = hex::decode(cfg_snapshot.mesh.auth_key.trim()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid mesh.auth_key: {e}"),
            )
        })?;
        let blocklist = mesh_blocklist
            .as_ref()
            .cloned()
            .ok_or_else(|| std::io::Error::other("mesh enabled without a blocklist"))?;
        Some(
            MeshHandle::bind(cfg_snapshot.mesh.node_id, blocklist, listen, peers, key)
                .await
                .map_err(|e| std::io::Error::other(format!("mesh bind failed: {e}")))?,
        )
    } else {
        None
    };
    #[cfg(not(feature = "mesh"))]
    if cfg_snapshot.mesh.enabled {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mesh.enabled=true but this binary was built without the mesh feature",
        ));
    }

    let mut enforcement = EnforcementService::new(
        store.clone(),
        metrics.clone(),
        xdp_box,
        enforcement_shutdown.clone(),
    );
    #[cfg(feature = "mesh")]
    if let Some(mesh) = mesh_blocklist {
        enforcement = enforcement.with_mesh_blocklist(mesh);
    }
    #[cfg(feature = "mesh")]
    if let Some(handle) = mesh_handle {
        enforcement = enforcement.with_mesh_handle(handle);
    }
    // Checkpoint coordination: barrier + deadline mirror shared with the
    // periodic checkpoint loop (PATCH 7/8: build_snapshot consumes real state).
    let checkpoint_shared = Arc::new(CheckpointShared::new());
    enforcement = enforcement.with_checkpoint_shared(checkpoint_shared.clone());
    // Lift WAL handle into pipeline scope for checkpoint loop.
    let mut pipeline_wal: Option<Arc<Wal>> = None;
    // Snapshot-derived CIDR/expiration seeds (None = no snapshot used).
    let mut snapshot_seed: Option<SnapshotRestore> = None;
    // Crash-durable block state: open WAL, load snapshot if exists, replay tail.
    if cfg_snapshot.wal.enabled {
        let hard_wal = !cfg_snapshot.wal.allow_volatile_fallback;
        match Wal::open(
            &cfg_snapshot.wal.dir,
            cfg_snapshot.wal.compress,
            cfg_snapshot.wal.durability,
            cfg_snapshot.wal.seg_max_bytes,
            cfg_snapshot.wal.retention_max_bytes,
        ) {
            Ok(wal) => {
                let wal = Arc::new(wal);

                // Load checkpoint snapshot if available.
                // snapshot_lsn is the WAL boundary the snapshot captures
                // (all entries <= snapshot_lsn are reflected). ckpt_lsn is
                // the Checkpoint record's own LSN (used for retention).
                let snap_lsn = wal.snapshot_lsn();
                let _ckpt_lsn = wal.ckpt_lsn();
                let snapshot_used = if snap_lsn > 0 {
                    let snap_path = snapshot_path(&cfg_snapshot.wal.dir, snap_lsn);
                    match load_snapshot(&snap_path) {
                        Ok(Some(snap)) => {
                            info!(
                                "Checkpoint snapshot found (snap_lsn={}) — replaying tail only",
                                snap_lsn
                            );
                            snapshot_seed = Some(restore_from_snapshot(&store, &snap));
                            true
                        }
                        _ => {
                            tracing::warn!(
                                "MANIFEST snap_lsn={} but snapshot missing — full WAL replay",
                                snap_lsn
                            );
                            false
                        }
                    }
                } else {
                    false
                };

                // Hard-WAL invariant (PATCH 9): snapshot present but the WAL
                // history it relies on has been pruned → recovery would start
                // from a hole. Fail closed under allow_volatile_fallback=false.
                if snapshot_used
                    && hard_wal
                    && let Some(oldest) = wal.oldest_lsn()
                    && oldest > snap_lsn + 1
                {
                    return Err(std::io::Error::other(format!(
                        "WAL history pruned past checkpoint boundary (snapshot_lsn={snap_lsn}, oldest_lsn={oldest}) — cannot reconstruct state (allow_volatile_fallback=false)"
                    )));
                }
                // Seed-fold (PATCH 9): the tail fold STARTS from snapshot
                // block state, so snapshot IPs untouched by the tail remain
                // blocked, and a tail UnblockIp correctly removes them.
                // Pre-cleanup of snapshot IPs is implicit: restore_from_snapshot
                // records are overwritten by the fold result below.
                let mut replay_seed: std::collections::HashMap<
                    std::net::IpAddr,
                    (String, u64, u64),
                > = std::collections::HashMap::new();
                if let Some(seed) = snapshot_seed.as_ref() {
                    // Seed from the exact snapshot record. Do not replace the
                    // historical reason/since timestamp, and do not reconstruct
                    // an absolute deadline from a rounded remaining TTL.
                    for (ip, reason, since_ns, expires_at_ns) in seed.ip_states.iter() {
                        replay_seed
                            .insert(*ip, (reason.clone(), *since_ns, expires_at_ns.unwrap_or(0)));
                    }
                }

                let min_lsn = if snapshot_used { snap_lsn } else { 0u64 };

                let restored_ttls =
                    match replay_wal_into_store_seeded(&store, &wal, min_lsn, replay_seed) {
                        Ok(r) => {
                            info!(
                                "WAL replay: restored {} IP blocks (min_lsn={})",
                                r.len(),
                                min_lsn
                            );
                            r
                        }
                        Err(e) => {
                            tracing::error!("WAL replay: {}", e);
                            if hard_wal {
                                return Err(std::io::Error::other(format!(
                                    "WAL replay failed (allow_volatile_fallback=false): {e}"
                                )));
                            }
                            vec![]
                        }
                    };
                // CIDR recovery is seeded from the snapshot exactly like IP
                // recovery. This is required for the invariant:
                // full WAL replay == snapshot + WAL tail replay. In particular,
                // a tail UnblockCidr must remove a CIDR that existed in the
                // checkpoint image.
                let cidr_seed = snapshot_seed
                    .as_ref()
                    .map(|seed| {
                        seed.cidr_states
                            .iter()
                            .copied()
                            .collect::<std::collections::HashMap<_, _>>()
                    })
                    .unwrap_or_default();
                let (restored_cidrs, final_cidrs) =
                    match replay_wal_cidrs_seeded(&wal, min_lsn, cidr_seed) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::error!("WAL CIDR replay: {}", e);
                            if hard_wal {
                                return Err(std::io::Error::other(format!(
                                    "WAL CIDR replay failed (allow_volatile_fallback=false): {e}"
                                )));
                            }
                            (vec![], std::collections::HashSet::new())
                        }
                    };
                if snapshot_used {
                    let snapshot_cidrs = snapshot_seed
                        .as_ref()
                        .map(|s| {
                            s.cidr_states
                                .iter()
                                .map(|(n, _)| *n)
                                .collect::<std::collections::HashSet<_>>()
                        })
                        .unwrap_or_default();
                    for network in snapshot_cidrs.difference(&final_cidrs) {
                        enforcement.remove_restored_cidr(*network);
                    }
                }
                if !restored_cidrs.is_empty() {
                    info!("WAL replay: restored {} CIDR blocks", restored_cidrs.len());
                }
                enforcement = enforcement.with_wal(Arc::clone(&wal));
                // Re-arm TTL ring and CIDR index with restored state.
                // PATCH 5: independent domains — a snapshot with 0 temporary
                // IPs + 3 CIDRs must still restore CIDRs (no coupling via one
                // !restored_ttls.is_empty() condition).
                if let Some(seed) = snapshot_seed.take() {
                    // Snapshot path: re-arm from absolute deadlines captured in
                    // the snapshot. remaining_secs = ceil(deadline - now).
                    if !seed.ip_expirations.is_empty() {
                        let now_ns = crate::engine::checkpoint::now_unix_ns();
                        let pairs = seed.ip_expirations.iter().map(|(ip, deadline)| {
                            (*ip, deadline.saturating_sub(now_ns).div_ceil(1_000_000_000))
                        });
                        enforcement.restore_expirations(pairs);
                    }
                    if !seed.cidr_expirations.is_empty() {
                        let now_ns = crate::engine::checkpoint::now_unix_ns();
                        let pairs = seed
                            .cidr_expirations
                            .iter()
                            .filter(|(net, _)| final_cidrs.contains(net))
                            .map(|(net, deadline)| {
                                (
                                    *net,
                                    deadline.saturating_sub(now_ns).div_ceil(1_000_000_000),
                                )
                            });
                        enforcement.restore_cidr_blocks(pairs);
                    }
                }
                if !restored_ttls.is_empty() {
                    enforcement.restore_expirations(restored_ttls);
                }
                if !restored_cidrs.is_empty() {
                    enforcement.restore_cidr_blocks(restored_cidrs);
                }
                pipeline_wal = Some(wal);
            }
            Err(e) => {
                tracing::error!("WAL open failed ({}): {}", cfg_snapshot.wal.dir, e);
                if hard_wal {
                    return Err(std::io::Error::other(format!(
                        "WAL open failed ({}): {e}",
                        cfg_snapshot.wal.dir
                    )));
                }
            }
        }
    }
    let mut shutdown_rx = engine.shutdown_rx();
    // Spawn the enforcement actor and keep the JoinHandle. Do not await run()
    // here — that starved detection, native ingest, SYNPROXY, and IPC.
    // Crash is observed in the main select below (Gotcha C).
    let mut enforcement_handle = tokio::spawn(async move {
        if let Err(e) = enforcement.run(enforcement_rx).await {
            tracing::error!("enforcement actor: {e}");
        }
    });

    let detection = Arc::new(DetectionEngine::try_new(
        store.clone(),
        cfg_handle.clone(),
        engine.enforcement_tx.clone(),
        metrics.clone(),
        engine.shutdown.clone(),
    )?);
    let event_tx = detection.event_sender();
    if cfg_snapshot.native_ingest.enabled
        && let Err(e) = crate::engine::native::spawn(
            cfg_snapshot.native_ingest.interface.clone(),
            cfg_snapshot.native_ingest.max_events_per_sec,
            cfg_snapshot.xdp.trusted_overlay_cidrs.clone(),
            event_tx.clone(),
            engine.shutdown.clone(),
        )
    {
        return Err(std::io::Error::other(format!(
            "native ingest start failed: {e}"
        )));
    }
    if cfg_snapshot.synproxy.enabled
        && let Err(e) =
            crate::engine::synproxy::install(&cfg_snapshot.synproxy, &cfg_snapshot.xdp.interface)
    {
        engine.shutdown.store(true, Ordering::Release);
        return Err(std::io::Error::other(format!("synproxy setup failed: {e}")));
    }
    let detection_started = detection
        .clone()
        .spawn_workers(cfg_snapshot.engine.worker_threads);
    if !detection_started {
        engine.pipeline_failed.store(true, Ordering::Release);
        engine.shutdown.store(true, Ordering::Release);
        if cfg_snapshot.synproxy.enabled {
            let _ = crate::engine::synproxy::uninstall();
        }
        return Err(std::io::Error::other(
            "detection workers failed to start; refusing ready state",
        ));
    }
    *engine.detection.lock().unwrap_or_else(|e| e.into_inner()) = Some(detection.clone());

    let upstream_cfg = Arc::new(cfg_snapshot.clone());
    let upstream_metrics = metrics.clone();
    let upstream_shutdown = engine.shutdown_rx();
    tokio::spawn(async move {
        crate::engine::upstream::run(upstream_cfg, upstream_metrics, upstream_shutdown).await;
    });

    let mut forecaster = if cfg_snapshot.forecasting.enabled {
        let forecaster = Arc::new(Forecaster::new(
            store.clone(),
            cfg_snapshot.forecasting.clone(),
            engine.enforcement_tx.clone(),
            metrics.clone(),
        ));
        let fc = forecaster.clone();
        let mut fc_rx = engine.shutdown_rx();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = fc.run() => {}
                _ = fc_rx.changed() => {
                    tracing::info!("forecaster: shutdown signal received");
                }
            }
        });
        Some((forecaster, handle))
    } else {
        // P1-9: honoring `forecasting.enabled` — previously the flag was
        // read nowhere and the span always ran (queue feed kept growing).
        tracing::info!(
            "forecasting.enabled=false — forecaster span not started (threat_sample queue not fed)"
        );
        None
    };

    // Capture for checkpoint loop before store is moved into server.
    let store_arc = store.clone();
    let cfg_dir = cfg_snapshot.wal.dir;

    let server = match crate::ipc::server::IpcServer::bind(
        cfg_handle.clone(),
        engine.clone(),
        event_tx,
        store,
        engine.enforcement_tx.clone(),
    )
    .await
    {
        Ok(server) => server,
        Err(e) => {
            if cfg_snapshot.synproxy.enabled {
                let _ = crate::engine::synproxy::uninstall();
            }
            engine.shutdown.store(true, Ordering::Release);
            return Err(e);
        }
    };
    // Pipeline is genuinely serving: enforcement, detection, IPC all running.
    // boot_pipeline blocks in select! below until shutdown, so this is the
    // one place readiness can be observed while the daemon is live.
    engine.pipeline_ready.store(true, Ordering::Release);
    // Signal startup readiness to main BEFORE blocking on shutdown select!.
    // boot_pipeline only returns when shutdown fires, so we send here.
    {
        let mut startup_tx_guard = engine.startup_tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = startup_tx_guard.take() {
            tx.send(Ok(())).ok();
        }
    }

    // Periodic checkpoint loop: snapshot state every CHECKPOINT_INTERVAL.
    if let Some(wal) = pipeline_wal {
        let engine = engine.clone();
        std::mem::drop(tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(300);
            let mut tick = tokio::time::interval(interval);
            tick.tick().await; // skip immediate first fire (empty snapshot)
            let mut sd = engine.shutdown_rx();
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = sd.changed() => {
                        info!("checkpoint loop: shutdown");
                        break;
                    }
                }
                // The boundary and the complete snapshot image must be
                // captured while holding the same barrier used by enforcement
                // for [WAL append + Store mutation]. Do not move build_snapshot
                // outside this guard: it reads the Store, so capturing only the
                // LSN under the guard is not sufficient.
                let (boundary, snap) = {
                    let _guard = checkpoint_shared
                        .barrier
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());

                    let boundary = wal.begin_checkpoint();
                    let mirror = checkpoint_shared.state();
                    // CIDR deadlines: mirror covers temporaries; permanent
                    // CIDRs (in store.active_cidrs but absent from the mirror)
                    // emit None.
                    let cidrs: Vec<CidrSnapshot> = store_arc
                        .active_cidrs
                        .iter()
                        .map(|e| *e.key())
                        .map(|network| CidrSnapshot {
                            network,
                            expires_at_ns: mirror.cidr_expirations.get(&network).copied(),
                        })
                        .collect();
                    let ckpt_state = CheckpointState {
                        ip_expirations: mirror.ip_expirations.clone(),
                        cidrs,
                    };
                    let snap = build_snapshot(&store_arc, &ckpt_state, boundary.lsn);
                    (boundary, snap)
                };

                // Filesystem I/O deliberately happens after the barrier is
                // released. The barrier protects logical consistency, not disk I/O.
                let snap_path = match write_snapshot(&cfg_dir, &snap, boundary.lsn) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("snapshot write: {}", e);
                        continue;
                    }
                };
                match wal.finish_checkpoint(boundary.lsn) {
                    Ok(new_lsn) => info!(
                        "checkpoint (lsn={} snap_lsn={}): {}",
                        new_lsn, boundary.lsn, snap_path
                    ),
                    Err(e) => tracing::error!("WAL checkpoint: {}", e),
                }
            }
        }));
    }

    // Graceful shutdown: wait for signal, then join tasks.
    // Gotcha C: a detached actor fails silently — select against the
    // enforcement JoinHandle so a crash becomes FATAL, not a hang.
    tokio::select! {
        res = &mut enforcement_handle => {
            tracing::error!(result = ?res, "enforcement actor terminated unexpectedly");
            engine.pipeline_failed.store(true, Ordering::Release);
            return Err(std::io::Error::other("enforcement actor terminated; pipeline FATAL"));
        }
        _ = server.start() => {}
        _ = shutdown_rx.changed() => {
            tracing::info!("pipeline: shutdown signal received, draining...");
            enforcement_shutdown.store(true, Ordering::Release);
            // Join with timeout to avoid hanging on stuck tasks.
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                tracing::warn!("enforcement shutdown timed out via rx changed");
                            }
                            _ = tokio::time::sleep_until(deadline) => {
                                tracing::warn!("enforcement shutdown timed out");
                            }
                        }
            if let Some((_, handle)) = forecaster.as_mut() {
                tokio::select! {
                    _ = handle => {}
                    _ = tokio::time::sleep_until(deadline) => {
                        tracing::warn!("forecaster shutdown timed out");
                    }
                }
            }
        }
    }
    if cfg_snapshot.synproxy.enabled
        && let Err(e) = crate::engine::synproxy::uninstall()
    {
        tracing::warn!(error = %e, "failed to remove RamShield SYNPROXY ruleset during shutdown");
    }
    Ok(())
}
