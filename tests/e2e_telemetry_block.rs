//! Part 2 end-to-end gate: telemetry over the REAL IPC wire → detection →
//! enforcement → store/WAL. A burst of report_connections frames for one IP
//! must land a block in the store without any test-level plumbing — the same
//! path an operator's proxy/sidecar uses in production.
//!
//! Skipped by design: a real HTTP reverse proxy is environment, not code —
//! the proxy templates now point at the app backend and telemetry is fed via
//! this exact wire protocol (see QUICKSTART Step 4).

use ramshield::{Config, Engine};
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn report_connections_burst_blocks_the_ip_end_to_end() {
    let mut cfg = Config::default();
    // Ephemeral port: fixed 17897 collides when the full workspace suite runs
    // other IPC tests concurrently (suite flake, not product defect).
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let ipc_addr = probe.local_addr().expect("local_addr");
    drop(probe);
    cfg.ipc.tcp_addr = ipc_addr.to_string();
    cfg.xdp.enabled = false; // safe mode: in-band enforcement, no kernel attach
    let wal_dir = std::env::temp_dir().join(format!(
        "rs_e2e_wal_{}_{}",
        std::process::id(),
        ipc_addr.port()
    ));
    let _ = std::fs::remove_dir_all(&wal_dir);
    std::fs::create_dir_all(&wal_dir).unwrap();
    cfg.wal.dir = wal_dir.to_str().unwrap().into();

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg, store.clone(), metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let target: IpAddr = "203.0.113.7".parse().unwrap();
    // 600 events > emergency_burst_threshold (500): the per-worker fast path
    // fires before any periodic flush.
    let event = r#"{"ip":"203.0.113.7","bytes":512,"status_code":404,"proto_fp":42}"#;
    let frame = format!(
        r#"{{"type":"report_connections","events":[{}]}}"#,
        vec![event; 200].join(",")
    );

    let mut stream = TcpStream::connect(ipc_addr).expect("TCP connect to IPC");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    for _ in 0..3 {
        writeln!(stream, "{frame}").expect("write frame");
    }
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");
    let v: serde_json::Value = serde_json::from_str(&line).expect("valid JSON response");
    assert_eq!(v["type"], "batch_ok", "telemetry must be accepted: {line}");
    assert_eq!(
        v["accepted"], 200,
        "every event in the frame must be accepted"
    );

    // Block must become observable in the store (enforcement sink) within a
    // few hundred ms of the emergency fast path firing.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut blocked = false;
    while Instant::now() < deadline {
        if let Some(rec) = store.get(&target)
            && rec.is_blocked()
        {
            blocked = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(blocked, "telemetry burst must end in a store-level block");

    engine.shutdown();
    let _ = std::fs::remove_dir_all(&wal_dir);
}
