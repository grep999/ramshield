/// A-037 Core benchmarks and regression budgets for ramshield-protocol.
/// Establishes reproducible baselines for protocol operations under load.
use criterion::{Criterion, criterion_group, criterion_main};
use ramshield_protocol::{Message, Request, ReplayStore, sign};
use std::time::Duration;

/// Benchmark Message serialization - measures encoding throughput
fn bench_message_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_serialize");
    let msg = Message::request(Request::BlockIp {
        ip: "10.0.0.1".into(),
        reason: "test".into(),
        ttl_secs: Some(3600),
    });
    group.bench_function("serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(&msg);
        });
    });
    group.finish();
}

/// Benchmark HMAC sign - measures signature throughput
fn bench_hmac_sign(c: &mut Criterion) {
    let mut group = c.benchmark_group("hmac_sign");
    group.bench_function("sign", |b| {
        b.iter(|| {
            let _ = sign(b"test-key-16-bytes", "k1", 42, b"test-message");
        });
    });
    group.finish();
}

/// Benchmark replay store check - measures admission throughput
fn bench_replay_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay_store");
    let store = ReplayStore::new(1000, Duration::from_secs(60));
    group.bench_function("check_and_record", |b| {
        b.iter(|| {
            let _ = store.check_and_record("k1", &[0u8; 32]);
        });
    });
    group.finish();
}

criterion_group!(
    protocol_benches,
    bench_message_serialize,
    bench_hmac_sign,
    bench_replay_store
);
criterion_main!(protocol_benches);
