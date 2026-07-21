// RamShield — per-module micro-benchmarks.
//
// Run:  cargo bench --bench module_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ramshield::{
    cache::Cache,
    detection::{BloomFilter, ConnectionEvent},
    storage::{Store, Value},
};
use std::net::IpAddr;
use std::time::Duration;

// —— helpers —————————————————————————————————————————————

fn make_event(ip: IpAddr) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: 1_000_000_000,
        bytes: 512,
        status_code: 200,
        proto_fingerprint: 0,
    }
}

#[allow(dead_code)]
fn make_event_at(i: usize) -> ConnectionEvent {
    make_event(random_ip(i))
}

fn random_ip(i: usize) -> IpAddr {
    let a = ((i % 223) + 1) as u8;
    let b = ((i / 223) % 256) as u8;
    let c = ((i / 57_000) % 256) as u8;
    let d = ((i / 14_560_000) % 254 + 1) as u8;
    IpAddr::from([a, b, c, d])
}

// —— 1. Storage benchmarks ——————————————————————————————————————————

fn bench_store_insert(c: &mut Criterion) {
    c.bench_function("store_insert_unique", |b| {
        let store = Store::new(10_000_000);
        let mut i = 0usize;
        let ram_limit = 64 * 1024 * 1024;
        b.iter(|| {
            let ip = random_ip(i);
            let key = ip.to_string();
            let _ = store.insert(key, Value::from_bytes(&[0u8; 48]), None, ram_limit);
            i += 1;
            black_box(&store);
        });
    });

    c.bench_function("store_insert_replace", |b| {
        let store = Store::new(10_000_000);
        let ram_limit = 64 * 1024 * 1024;
        let _ = store.insert(
            "1.2.3.4".into(),
            Value::from_bytes(&[0u8; 48]),
            None,
            ram_limit,
        );
        b.iter(|| {
            let _ = store.insert(
                "1.2.3.4".into(),
                Value::from_bytes(&[0u8; 48]),
                None,
                ram_limit,
            );
        });
    });

    c.bench_function("store_increment", |b| {
        let store = Store::new(10_000_000);
        let mut i = 0usize;
        b.iter(|| {
            let ip = random_ip(i);
            black_box(store.increment(&ip.to_string(), 1));
            i += 1;
        });
    });

    c.bench_function("store_at_capacity", |b| {
        let ram_limit = 100;
        b.iter(|| {
            let store = Store::new(100);
            for i in 0..100 {
                let _ = store.insert(
                    format!("10.0.0.{}", i),
                    Value::from_bytes(&[0u8; 48]),
                    None,
                    ram_limit,
                );
            }
            // 101st insert should fail
            let res = store.insert(
                "10.0.1.0".into(),
                Value::from_bytes(&[0u8; 48]),
                None,
                ram_limit,
            );
            black_box(res.is_err());
        });
    });
}

// —— 2. Detection / Bloom —————————————————————————————————————————————

fn bench_bloom(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter");
    for size in [1_024, 8_192, 65_536, 524_288].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut bf = BloomFilter::new(size);
            let hit = IpAddr::from([10, 0, 1, 1]);
            let miss = IpAddr::from([192, 168, 1, 1]);
            for i in 0..(size / 4) {
                let ip = IpAddr::from([10, 0, ((i / 256) % 256) as u8, (i % 256) as u8]);
                bf.insert(ip);
            }
            b.iter(|| {
                black_box(bf.contains(hit));
                black_box(bf.contains(miss)); // likely miss
            });
        });
    }
    group.finish();
}

// —— 3. Cache ——————————————————————————————————————————————

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
            black_box(
                cache
                    .get(&"missingkey".to_string())
                    .map(|e| e.value.clone()),
            );
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

// —— 4. IPC serde round-trip ——————————————————————————————————————————

fn bench_ipc_serde(c: &mut Criterion) {
    // IpcRequest is not pub-re-exported from the lib root, so we do a
    // lightweight serde_json::Value round-trip instead, which mirrors the
    // actual hot path in the IPC server (serde_json::from_str → handle →
    // serde_json::to_string).

    let single =
        r#"{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}"#;
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

// —— group setup ———————————————————————————————————————————————

criterion_group!(
    benches,
    bench_store_insert,
    bench_bloom,
    bench_cache,
    bench_ipc_serde,
);
criterion_main!(benches);
