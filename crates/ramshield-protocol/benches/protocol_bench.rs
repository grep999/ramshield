/// A-037 Core benchmarks and regression budgets for ramshield-protocol.
/// Establishes reproducible baselines for protocol operations under load.
use criterion::{Criterion, criterion_group, criterion_main};
use ramshield_protocol::auth::{ReplayStore, sign};
use ramshield_protocol::{Message, Request};
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
