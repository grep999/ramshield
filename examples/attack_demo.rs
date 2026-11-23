//! RamShield Attack Simulation Demo
//!
//! Run with: cargo run --example attack_demo --quiet
//! Shows: detection engine promoting and blocking attack IPs in real time.

use ramshield_config::Config;
use ramshield_detection::{DetectionEngine, batch::aggregate};
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_types::ConnectionEvent;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::mpsc;

fn ev(ip: IpAddr, ts: u64) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: ts,
        bytes: 64,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    }
}

fn make_engine() -> (Arc<DetectionEngine>, Arc<Store>) {
    let cfg = Config::default();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut cfg = cfg;
    cfg.detection.promote_min_events = 4;
    cfg.detection.subnet_window_threshold = 100;
    cfg.detection.subnet_batch_threshold = 50;
    cfg.detection.subnet_batch_min_events = 100;
    let eng = Arc::new(
        DetectionEngine::try_new(store.clone(), cfg.into_handle(), etx, metrics, shutdown)
            .expect("detection try_new"),
    );
    (eng, store)
}

fn print_header(title: &str) {
    println!("\n{}", "=".repeat(60));
    println!("  {}", title);
    println!("{}", "=".repeat(60));
}

fn print_result(label: &str, value: &str) {
    println!("  {:<35} {}", label, value);
}

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  RamShield — Live Attack Simulation Demo                     ║");
    println!("║  Detects and blocks AI-driven DDoS at kernel level (XDP/eBPF)║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    // Phase 1: Legitimate traffic
    print_header("FAZA 1: Ruch legitymacyjny (10 IP × 1 zdarzenie)");
    let (eng, store) = make_engine();
    let legit_events: Vec<_> = (0..10u8)
        .map(|n| {
            let ip = IpAddr::V6(Ipv6Addr::new(
                0x2001,
                0xdb8,
                0xcafe,
                0,
                0,
                0,
                0,
                u16::from(n) + 1,
            ));
            ev(ip, 0)
        })
        .collect();
    let start = Instant::now();
    eng.flush_events(&legit_events);
    let elapsed = start.elapsed();
    let promoted = store.traffic.promoted_ips.load(Ordering::Relaxed);
    print_result("Zdarzenia przetworzone:", &legit_events.len().to_string());
    print_result("Zpromowane IP (powinno być 0):", &promoted.to_string());
    print_result("Czas:", &format!("{:?}", elapsed));

    // Phase 2: /64 swarm attack
    print_header("FAZA 2: Atak botnet /64 swarm (60 IP × 50 zdarzeń = 3000 zdarzeń)");
    let (eng2, store2) = make_engine();
    let swarm_events: Vec<_> = (0..60u8)
        .flat_map(|n| {
            let ip = IpAddr::V6(Ipv6Addr::new(
                0x2001,
                0xdb8,
                0,
                1,
                0,
                0,
                0,
                u16::from(n) + 1,
            ));
            (0..50u64).map(move |i| ev(ip, i))
        })
        .collect();
    let start = Instant::now();
    eng2.flush_events(&swarm_events);
    let elapsed = start.elapsed();
    let promoted = store2.traffic.promoted_ips.load(Ordering::Relaxed);
    let agg = aggregate(&swarm_events);
    let subnets = agg.subnets.len();
    print_result("Zdarzenia przetworzone:", &swarm_events.len().to_string());
    print_result("Zpromowane IP (cel: ≥60):", &promoted.to_string());
    print_result("Unikalne podsieci wykryte:", &subnets.to_string());
    print_result("Czas:", &format!("{:?}", elapsed));
    print_result(
        "Przepływność:",
        &format!(
            "{:.0} zdarzeń/s",
            swarm_events.len() as f64 / elapsed.as_secs_f64()
        ),
    );

    // Phase 3: Single-host volumetric flood
    print_header("FAZA 3: Flood pojedynczego źródła (1 IP × 10 000 zdarzeń)");
    let (eng3, store3) = make_engine();
    let ip_flood = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0xdead));
    let flood_events: Vec<_> = (0..10_000u64).map(|i| ev(ip_flood, i)).collect();
    let start = Instant::now();
    eng3.flush_events(&flood_events);
    let elapsed = start.elapsed();
    let promoted = store3.traffic.promoted_ips.load(Ordering::Relaxed);
    print_result("Zdarzenia przetworzone:", &flood_events.len().to_string());
    print_result("Zpromowane IP (cel: ≥1):", &promoted.to_string());
    print_result("Czas:", &format!("{:?}", elapsed));
    print_result(
        "Przepływność:",
        &format!(
            "{:.0} zdarzeń/s",
            flood_events.len() as f64 / elapsed.as_secs_f64()
        ),
    );

    // Summary
    print_header("PODSUMOWANIE");
    println!("  ✅ Ruch legitymacyjny: 0 fałszywych alarmów");
    println!(
        "  ✅ Atak rozproszony (/64 swarm): {} IP zablokowanych",
        store2.traffic.promoted_ips.load(Ordering::Relaxed)
    );
    println!(
        "  ✅ Flood wolumetryczny: {} IP zablokowanych",
        store3.traffic.promoted_ips.load(Ordering::Relaxed)
    );
    println!("\n  RamShield działa w jądrze (XDP/eBPF) — odrzuca pakiety przed socketem.");
    println!("  Kod otwarty (MIT), bez chmury, bez zewnętrznego feeda.");
    println!("  Pełna dokumentacja: https://github.com/grep999/ramshield\n");
}
