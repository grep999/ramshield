// RamShield — per-module micro-benchmarks.
//
// Run:  cargo bench --bench module_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ramshield::{
    cache::Cache,
    detection::{Batch, BloomFilter, ConnectionEvent, RateTracker},
    storage::Store,
};
use std::time::Duration;

// ── helpers ──────────────────────────────────────────────────────────────

fn make_event(ip: &str) -> ConnectionEvent {
    ConnectionEvent {
        ip: ip.to_string(),
        timestamp_ns: 1_000_000_000,
        bytes: 512,
        status_code: 200,
        proto_fingerprint: 0,
    }
}

fn random_ip(i: usize) -> String {
    let a = (i % 223) + 1;
    let b = (i / 223) % 256;
    let c = (i / 57_000) % 256;
    let d = (i / 14_560_000) % 254 + 1;
    format!("{}.{}.{}.{}", a, b, c, d)
}

// ── 1. Storage benchmarks ────────────────────────────────────────────────

fn bench_store_insert(c: &mut Criterion) {
    c.bench_function("store_insert_unique", |b| {
        let store = Store::new(10_000_000);
        let mut i = 0usize;
        b.iter(|| {
            let ip = random_ip(i);
            let _ = store.insert(ip, make_event(&format!("{}", i)));
            i += 1;
            black_box(&store);
        });
    });

    c.bench_function("store_insert_replace", |b| {
        let store = Store::new(10_000_000);
        let _ = store.insert("1.2.3.4".into(), make_event("1.2.3.4"));
        b.iter(|| {
            let _ = store.insert("1.2.3.4".into(), make_event("1.2.3.4"));
        });
    });

    c.bench_function("store_block_ip", |b| {
        let store = Store::new(10_000_000);
        let mut i = 0usize;
        b.iter(|| {
            let ip = random_ip(i);
            store.block_ip(&ip, "benchmark", Some(3600));
            i += 1;
        });
    });

    c.bench_function("store_is_blocked_hit", |b| {
        let store = Store::new(10_000_000);
        store.block_ip("9.9.9.9", "test", Some(3600));
        b.iter(|| {
            black_box(store.is_blocked("9.9.9.9"));
        });
    });

    c.bench_function("store_is_blocked_miss", |b| {
        let store = Store::new(10_000_000);
        b.iter(|| {
            black_box(store.is_blocked("0.0.0.0"));
        });
    });

    c.bench_function("store_at_capacity", |b| {
        b.iter(|| {
            let store = Store::new(100);
            for i in 0..100 {
                let _ = store.insert(format!("10.0.0.{}", i), make_event(&format!("{}", i)));
            }
            // 101st insert should fail
            let res = store.insert("10.0.1.0".into(), make_event("x"));
            black_box(res.is_err());
        });
    });
}

// ── 2. Detection / Batch / Bloom / RateTracker ─────────────────────────

fn bench_batch(c: &mut Criterion) {
    c.bench_function("batch_add_event", |b| {
        let mut batch = Batch::new();
        b.iter(|| {
            batch.add_event(make_event("1.2.3.4"));
        });
    });

    c.bench_function("batch_fill_4096_flush", |b| {
        b.iter(|| {
            let mut batch = Batch::new();
            for i in 0..4096 {
                batch.add_event(make_event(&random_ip(i)));
            }
            batch.clear();
            black_box(&batch);
        });
    });
}

fn bench_bloom(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter");
    for size in [1_024, 8_192, 65_536, 524_288].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut bf = BloomFilter::new(size);
            for i in 0..(size / 4) {
                bf.add(&format!("10.0.{}.{}", i / 256, i % 256));
            }
            b.iter(|| {
                black_box(bf.contains("10.0.1.1"));
                black_box(bf.contains("192.168.1.1")); // likely miss
            });
        });
    }
    group.finish();
}

fn bench_rate_tracker(c: &mut Criterion) {
    c.bench_function("rate_tracker_update", |b| {
        let mut rt = RateTracker::new(1000);
        let mut t = 1u64;
        b.iter(|| {
            rt.record_request(t * 1_000_000, t);
            t += 1;
        });
    });

    c.bench_function("rate_tracker_should_block_cold", |b| {
        let rt = RateTracker::new(1000);
        b.iter(|| {
            black_box(rt.should_block(0));
        });
    });

    c.bench_function("rate_tracker_should_block_hot", |b| {
        let mut rt = RateTracker::new(1000);
        for i in 0..2000u64 {
            rt.record_request(i * 1_000_000, i);
        }
        b.iter(|| {
            black_box(rt.should_block(2000));
        });
    });
}

// ── 3. Cache ─────────────────────────────────────────────────────────────

fn bench_cache(c: &mut Criterion) {
    // Note: Cache spawns a background evictor thread. Each bench iteration
    // creates a fresh cache to avoid cross-bench interference.
    c.bench_function("cache_insert_new", |b| {
        let cache: Cache<String, String> = Cache::new(1_000_000, Duration::from_secs(3600));
        let mut i = 0usize;
        b.iter(|| {
            cache.insert(format!("key{}", i), "val".into(), None);
            i += 1;
        });
        cache.shutdown();
    });

    c.bench_function("cache_get_hit", |b| {
        let cache: Cache<String, String> = Cache::new(1_000_000, Duration::from_secs(3600));
        cache.insert("hotkey".into(), "val".into(), None);
        b.iter(|| {
            black_box(cache.get(&"hotkey".to_string()).map(|e| e.value.clone()));
        });
        cache.shutdown();
    });

    c.bench_function("cache_get_miss", |b| {
        let cache: Cache<String, String> = Cache::new(1_000_000, Duration::from_secs(3600));
        b.iter(|| {
            black_box(cache.get(&"missingkey".to_string()).map(|e| e.value.clone()));
        });
        cache.shutdown();
    });

    c.bench_function("cache_remove", |b| {
        let cache: Cache<String, String> = Cache::new(1_000_000, Duration::from_secs(3600));
        for i in 0..1000 {
            cache.insert(format!("k{}", i), "v".into(), None);
        }
        let mut i = 0usize;
        b.iter(|| {
            black_box(cache.remove(&format!("k{}", i % 1000)));
            i += 1;
        });
        cache.shutdown();
    });
}

// ── 4. IPC serde round-trip ──────────────────────────────────────────────

fn bench_ipc_serde(c: &mut Criterion) {
    // IpcRequest is not pub-re-exported from the lib root, so we do a
    // lightweight serde_json::Value round-trip instead, which mirrors the
    // actual hot path in the IPC server (serde_json::from_str → handle →
    // serde_json::to_string).

    let single = r#"{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}"#;
    let batch_json = format!(
        r#"{{"type":"report_connections","events":[{}]}}"#,
        (0..100)
            .map(|i| format!(r#"{{"ip":"10.0.0.{}","bytes":256,"status_code":200}}"#, i))
            .collect::<Vec<_>>()
            .join(",")
    );
    let stats_req = r#"{"type":"get_stats"}"#;

    c.bench_function("ipc_parse_single_event", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(single).unwrap();
            black_box(&v);
        });
    });

    c.bench_function("ipc_parse_batch_100", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(&batch_json).unwrap();
            black_box(&v);
        });
    });

    c.bench_function("ipc_parse_stats", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(stats_req).unwrap();
            black_box(&v);
        });
    });

    // Serialize a typical response
    let resp = serde_json::json!({
        "type": "batch_ok",
        "accepted": 100,
        "rejected": 0
    });
    c.bench_function("ipc_serialize_response", |b| {
        b.iter(|| {
            let s = serde_json::to_string(&resp).unwrap();
            black_box(&s);
        });
    });
}

// ── group setup ──────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_store_insert,
    bench_batch,
    bench_bloom,
    bench_rate_tracker,
    bench_cache,
    bench_ipc_serde,
);
criterion_main!(benches);