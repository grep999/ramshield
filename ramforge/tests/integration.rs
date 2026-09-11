use std::net::IpAddr;
use std::thread;
use std::time::{Duration, Instant};

use ramforge::{AuthEvent, BruteforceMonitor, DetectionConfig};

fn evt(ip: &str) -> AuthEvent {
    AuthEvent {
        ip: ip.parse().unwrap(),
        timestamp: Instant::now(), // fresh each call — cooldowns measure real time
        source: "test".into(),
        success: false,
        username: None,
    }
}

#[test]
fn block_reason_strings() {
    assert_eq!(ramforge::BlockReason::BruteForce.as_str(), "brute_force");
    assert_eq!(
        ramforge::BlockReason::BehavioralAnomaly.as_str(),
        "behavioral_anomaly"
    );
    assert_eq!(ramforge::BlockReason::Manual.as_str(), "manual");
}

#[test]
fn config_defaults_match_nist_baseline() {
    let cfg = DetectionConfig::default();
    assert_eq!(cfg.max_failures, 5);
    assert_eq!(cfg.window_secs, 60);
    assert_eq!(cfg.block_ttl_secs, 3600);
    assert_eq!(cfg.adaptive_floor, 3);
    assert_eq!(cfg.evict_idle_secs, 86_400);
}

#[test]
fn block_command_carries_context() {
    let (monitor, rx) = BruteforceMonitor::new(DetectionConfig::default());
    for _ in 0..4 {
        monitor.process_failure(&evt("192.168.1.50"));
    }
    assert!(monitor.process_failure(&evt("192.168.1.50"))); // 5th failure triggers

    let cmd = rx.try_recv().expect("block command on channel");
    assert_eq!(cmd.ip.to_string(), "192.168.1.50");
    assert_eq!(cmd.reason.as_str(), "brute_force");
    assert_eq!(cmd.ttl_secs, 3600);
    assert!(cmd.context.contains("failures in 60s from test"));
    assert!(cmd.context.contains("strikes: 1"));
    monitor.shutdown();
}

#[test]
fn cooldown_expiry_rearms_counter() {
    let cfg = DetectionConfig {
        max_failures: 3,
        adaptive_floor: 1,
        block_ttl_secs: 1,
        ..DetectionConfig::default()
    };
    let (m, _rx) = BruteforceMonitor::new(cfg);
    let ip: IpAddr = "203.0.113.77".parse().unwrap();
    let fresh = || AuthEvent {
        ip,
        timestamp: Instant::now(),
        source: "test".into(),
        success: false,
        username: None,
    };

    // Block #1: threshold 3 (3 − 0 strikes).
    m.process_failure(&fresh());
    m.process_failure(&fresh());
    assert!(m.process_failure(&fresh()));

    thread::sleep(Duration::from_millis(1100)); // cooldown expires

    // Block #2: threshold 2 (3 − 1 strike).
    assert!(!m.process_failure(&fresh()));
    assert!(m.process_failure(&fresh()));

    thread::sleep(Duration::from_millis(1100)); // cooldown expires

    // Block #3: threshold 1 = adaptive floor (3 − 2 strikes).
    assert!(m.process_failure(&fresh()));
    m.shutdown();
}