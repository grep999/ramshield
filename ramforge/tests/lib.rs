use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ramforge::{AuthEvent, BruteforceMonitor, DetectionConfig};

fn cfg() -> DetectionConfig {
    DetectionConfig {
        max_failures: 3,
        adaptive_floor: 1,
        block_ttl_secs: 1,
        ..DetectionConfig::default()
    }
}

/// Fresh event with current timestamp — critical for cooldown/window tests.
fn fresh(ip: &str) -> AuthEvent {
    AuthEvent {
        ip: ip.parse().unwrap(),
        timestamp: Instant::now(),
        source: "test".into(),
        success: false,
        username: None,
    }
}

#[test]
fn threshold_triggers_block() {
    let (m, _rx) = BruteforceMonitor::new(cfg());
    assert!(!m.process_failure(&fresh("192.168.1.100")));
    assert!(!m.process_failure(&fresh("192.168.1.100")));
    assert!(m.process_failure(&fresh("192.168.1.100")));
    m.shutdown();
}

#[test]
fn different_ips_independent() {
    let (m, _rx) = BruteforceMonitor::new(cfg());
    assert!(!m.process_failure(&fresh("10.0.0.1")));
    assert!(!m.process_failure(&fresh("10.0.0.1")));
    assert!(!m.process_failure(&fresh("10.0.0.2")));
    assert!(m.process_failure(&fresh("10.0.0.1")));
    assert!(!m.process_failure(&fresh("10.0.0.2")));
    m.shutdown();
}

#[test]
fn cooldown_prevents_repeat_block() {
    let (m, _rx) = BruteforceMonitor::new(cfg());
    let ip = "172.16.0.1";
    // Block #1: threshold 3.
    assert!(!m.process_failure(&fresh(ip)));
    assert!(!m.process_failure(&fresh(ip)));
    assert!(m.process_failure(&fresh(ip)));
    // During cooldown: no new block regardless of further failures.
    assert!(!m.process_failure(&fresh(ip)));
    assert!(!m.process_failure(&fresh(ip)));
    m.shutdown();
}

#[test]
fn escalating_strikes_lower_threshold() {
    let (m, _rx) = BruteforceMonitor::new(cfg()); // threshold 3, floor 1, ttl 1s
    let ip = "172.16.0.9";

    // Block #1: threshold 3 (0 strikes).
    assert!(!m.process_failure(&fresh(ip)));
    assert!(!m.process_failure(&fresh(ip)));
    assert!(m.process_failure(&fresh(ip)));
    thread::sleep(Duration::from_millis(1100)); // cooldown expires

    // Block #2: threshold 2 (1 strike). 2 failures needed.
    assert!(!m.process_failure(&fresh(ip))); // 1 fail, below threshold 2
    assert!(m.process_failure(&fresh(ip))); // 2 fails → block #2

    thread::sleep(Duration::from_millis(1100)); // cooldown expires

    // Block #3: threshold 1 (floor, 2 strikes). Single fresh failure blocks.
    assert!(m.process_failure(&fresh(ip))); // block #3
    m.shutdown();
}

#[test]
fn ipv6_support() {
    let (m, _rx) = BruteforceMonitor::new(cfg());
    assert!(!m.process_failure(&fresh("2001:db8::1")));
    assert!(!m.process_failure(&fresh("2001:db8::1")));
    assert!(m.process_failure(&fresh("2001:db8::1")));
    m.shutdown();
}

#[test]
fn concurrent_processing() {
    let (monitor, _rx) = BruteforceMonitor::new(cfg());
    let monitor = Arc::new(monitor);
    let blocked = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for i in 0..8 {
        let m = Arc::clone(&monitor);
        let b = Arc::clone(&blocked);
        let ip: IpAddr = format!("10.0.{}.{}", i / 256, i % 256).parse().unwrap();
        handles.push(thread::spawn(move || {
            for _ in 0..6 {
                let evt = AuthEvent {
                    ip,
                    timestamp: Instant::now(),
                    source: "concurrent".into(),
                    success: false,
                    username: None,
                };
                if m.process_failure(&evt) {
                    b.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // 8 IPs × 6 failures, threshold 3 → exactly one block each
    // (first 3 trigger, remaining 3 suppressed by cooldown)
    assert_eq!(blocked.load(Ordering::Relaxed), 8);
    monitor.shutdown();
}

#[test]
fn window_expiry_resets_count() {
    let c = DetectionConfig {
        max_failures: 3,
        window_secs: 1,
        block_ttl_secs: 3600,
        ..DetectionConfig::default()
    };
    let (m, _rx) = BruteforceMonitor::new(c);
    // 2 failures (below threshold) then let window slide.
    assert!(!m.process_failure(&fresh("192.168.1.10")));
    assert!(!m.process_failure(&fresh("192.168.1.10")));
    thread::sleep(Duration::from_millis(1100)); // window slides — count resets
    // Fresh window: single failure should not block (1 < threshold 3).
    assert!(!m.process_failure(&fresh("192.168.1.10")));
    m.shutdown();
}

#[test]
fn stats_snapshot() {
    let (m, _rx) = BruteforceMonitor::new(cfg());
    assert_eq!(m.snapshot_stats().ips_tracked, 0);
    assert!(!m.process_failure(&fresh("10.0.0.1")));
    assert!(!m.process_failure(&fresh("10.0.0.2")));
    let s = m.snapshot_stats();
    assert_eq!(s.ips_tracked, 2);
    assert_eq!(s.currently_blocked, 0);
    assert_eq!(s.total_failures, 2);
    m.shutdown();
}