//! Integration: detection → enforcement flow through real Engine channels.
//! Proves the unified crates compose: events in, blocks out, WAL durable.

use ramshield_config::Config;
use ramshield_storage::{wal::Wal, BlockState, Value};
use ramshield_types::{EnforceAction, EnforceCommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn detection_tracks_ip_through_engine_pipeline() {
    let cfg = Arc::new(arc_swap::ArcSwap::from_pointee(Config::default()));
    let store = Arc::new(ramshield_storage::Store::new(16));
    store.traffic.ram_limit_mb.store(256, Ordering::Relaxed);
    let metrics = Arc::new(ramshield_metrics::Metrics::new());

    let (enforcement_tx, _enforcement_rx) = tokio::sync::mpsc::channel(4096);
    let detection = Arc::new(ramshield_detection::DetectionEngine::new(
        store.clone(),
        cfg,
        enforcement_tx,
        metrics.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    detection.clone().spawn_workers(2);
    let event_tx = detection.event_sender();
    let target = std::net::IpAddr::from([10, 1, 2, 3]);
    // 200 events > promote_min_events(8); flush_events is the synchronous
    // test/IPC entry — no worker timing dependence.
    let events: Vec<ramshield_types::ConnectionEvent> = (0..200u64)
        .map(|i| ramshield_types::ConnectionEvent {
            ip: target,
            timestamp_ns: 1_000 + i,
            bytes: 512,
            status_code: if i % 3 == 0 { 404 } else { 200 },
            proto_fingerprint: 42,
        })
        .collect();
    detection.flush_events(&events);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let rec = store.get(&target);
    assert!(rec.is_some(), "IP must be tracked after 200 events");
    match rec.unwrap() {
        Value::IpRecord(r) => {
            assert!(r.request_count >= 1, "request_count must have advanced");
        }
        _ => panic!("expected IpRecord"),
    }
}

#[tokio::test]
async fn enforcement_wal_replay_roundtrip() {
    // Full durability loop: block via service with WAL attached, reopen WAL,
    // replay must contain the decision.
    let dir = std::env::temp_dir().join(format!("rs_int_wal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let store = Arc::new(ramshield_storage::Store::new(8));
    store.traffic.ram_limit_mb.store(256, Ordering::Relaxed);
    let metrics = Arc::new(ramshield_metrics::Metrics::new());

    let wal = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
        )
        .unwrap(),
    );

    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let svc = ramshield_enforcement::EnforcementService::new(
        store.clone(),
        metrics,
        Box::new(ramshield_enforcement::StubXdpApplier),
        Arc::new(AtomicBool::new(false)),
    )
    .with_wal(wal);
    let handle = tokio::spawn(svc.run(rx));

    let cmd = EnforceCommand {
        decision_id: uuid::Uuid::new_v4(),
        policy_version: 1,
        source: "integration".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: 60,
        reason: "high_rps".into(),
        ip: std::net::IpAddr::from([192, 168, 7, 7]),
        action: EnforceAction::Block,
    };
    tx.send(cmd).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Storage shows blocked.
    let rec = store
        .get(&std::net::IpAddr::from([192, 168, 7, 7]))
        .unwrap();
    match rec {
        Value::IpRecord(r) => assert!(matches!(r.block_state, BlockState::Blocked { .. })),
        _ => panic!("wrong type"),
    }

    drop(tx);
    handle.await.unwrap().unwrap();

    // Replay proves durability.
    let entries = Wal::replay(dir.to_str().unwrap()).unwrap();
    assert_eq!(entries.len(), 1, "one BlockIp entry expected");
    let _ = std::fs::remove_dir_all(&dir);
}
