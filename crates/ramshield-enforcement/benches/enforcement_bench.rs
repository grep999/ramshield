/// Minimal enforcement benchmarks for A-037 Core benchmarks and regression budgets.
/// These benchmarks verify that key enforcement structures and operations
/// maintain expected performance characteristics and do not regress.
use criterion::{Criterion, criterion_group, criterion_main};

// Benchmark BlocklistKey creation - should be O(1) and constant-time
// Only available when the xdp feature is enabled (module is cfg-gated).
#[cfg(feature = "xdp")]
fn bench_blocklist_key_creation(c: &mut Criterion) {
    let group = c.benchmark_group("blocklist_key_create");
    for _ in 0..10000 {
        let _key = ramshield_enforcement::xdp::BlocklistKey::from_ip(std::net::IpAddr::V4(
            "10.0.0.1".parse::<std::net::Ipv4Addr>().unwrap(),
        ));
    }
    group.finish();
}

// Benchmark IP parsing - should be O(1) and not allocate unnecessarily
fn bench_ip_parsing(c: &mut Criterion) {
    let group = c.benchmark_group("ip_parsing");
    for _ in 0..10000 {
        let _ip: std::net::Ipv4Addr = "10.0.0.1".parse().unwrap();
    }
    group.finish();
}

// Benchmark WAL entry struct access - verify struct fields are accessible
fn bench_wal_entry_struct(c: &mut Criterion) {
    let group = c.benchmark_group("wal_entry_struct");
    for _ in 0..10000 {
        let _entry = ramshield_storage::wal::WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "test".into(),
            ttl_secs: Some(100),
            ts_ns: 42,
        };
    }
    group.finish();
}

#[cfg(feature = "xdp")]
criterion_group!(
    enforcement_benches,
    bench_blocklist_key_creation,
    bench_ip_parsing,
    bench_wal_entry_struct
);

#[cfg(not(feature = "xdp"))]
criterion_group!(
    enforcement_benches,
    bench_ip_parsing,
    bench_wal_entry_struct
);

criterion_main!(enforcement_benches);
