use std::net::IpAddr;
use std::time::Instant;

macro_rules! bench {
    ($label:expr, $func:ident, $n:expr) => {
        for _ in 0..1000 {
            std::hint::black_box($func(100));
        }
        let avg_ns = $func($n);
        let ops_per_sec = 1_000_000_000.0 / avg_ns;
        println!(
            "  {:<48} {:>10.1} ns/op  ({:>10.0} ops/s)",
            $label, avg_ns, ops_per_sec
        );
    };
}

// ── Forecasting ──────────────────────────────────────────────────────────────

fn bench_hw_update(n: usize) -> f64 {
    let mut hw = ramshield_forecasting::HoltWinters::new(0.3, 0.1, 0.1, 60);
    for i in 0..60 {
        hw.update(1000.0 + (i as f64 * 10.0).sin() * 200.0);
    }
    let t0 = Instant::now();
    for i in 0..n {
        hw.update(1000.0 + ((i % 60) as f64 * 0.1).sin() * 200.0);
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_bayesian_update(n: usize) -> f64 {
    let mut bt = ramshield_forecasting::HypothesisTracker::new();
    for _ in 0..60 {
        bt.bayesian_update(0.5, 0.0, 0.1, false);
    }
    let t0 = Instant::now();
    for i in 0..n {
        bt.bayesian_update(if i % 10 == 0 { 3.0 } else { 0.5 }, -0.2, 0.3, i % 20 == 0);
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_bayesian_best_above(n: usize) -> f64 {
    let mut bt = ramshield_forecasting::HypothesisTracker::new();
    for _ in 0..100 {
        bt.bayesian_update(3.0, -0.5, 0.8, true);
    }
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(bt.best_above_threshold());
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

// ── Detection ────────────────────────────────────────────────────────────────

fn bench_ewma(n: usize) -> f64 {
    let mut prev = 1000.0f64;
    let t0 = Instant::now();
    for i in 0..n {
        prev =
            ramshield_detection::rate_tracker::ewma(prev, 1000.0 + (i as f64 * 0.01).sin() * 100.0);
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_cusum(n: usize) -> f64 {
    let mut s = 0.0f64;
    let t0 = Instant::now();
    for i in 0..n {
        let z = if i % 50 < 30 { 2.0 } else { 0.0 };
        s = ramshield_detection::rate_tracker::cusum_step_capped(s, z, 1.0, 10.0);
        std::hint::black_box(ramshield_detection::rate_tracker::cusum_fired(s, 5));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_pulse(n: usize) -> f64 {
    let mut prev_start = 0u64;
    let mut count = 0u8;
    let t0 = Instant::now();
    for i in 0..n {
        let (c, start, _) = ramshield_detection::rate_tracker::pulse_tracker_step(
            count,
            prev_start,
            i as u64 * 1_000_000,
            true,
            2,
            50,
        );
        count = c;
        prev_start = start;
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_bloom_insert(n: usize) -> f64 {
    let mut bloom = ramshield_detection::BloomFilter::new(8_000_000);
    let ips: Vec<IpAddr> = (0..n)
        .map(|i| {
            format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
                .parse()
                .unwrap()
        })
        .collect();
    let t0 = Instant::now();
    for ip in &ips {
        bloom.insert(*ip);
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_bloom_contains(n: usize) -> f64 {
    let mut bloom = ramshield_detection::BloomFilter::new(8_000_000);
    for i in 0..n {
        let ip: IpAddr = format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
            .parse()
            .unwrap();
        bloom.insert(ip);
    }
    let t0 = Instant::now();
    for i in 0..n {
        let ip: IpAddr = format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
            .parse()
            .unwrap();
        std::hint::black_box(bloom.contains(ip));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_batch_aggregate(n: usize) -> f64 {
    use ramshield_detection::batch::aggregate;
    use ramshield_types::ConnectionEvent;
    let events: Vec<ConnectionEvent> = (0..4096)
        .map(|i| ConnectionEvent {
            ip: format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
                .parse()
                .unwrap(),
            timestamp_ns: 1_000_000_000 + i as u64 * 1_000_000,
            bytes: 256,
            status_code: 200,
            proto_fingerprint: 1,
        })
        .collect();
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(aggregate(&events));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

// ── Storage ──────────────────────────────────────────────────────────────────

fn bench_subnet_key_v4(n: usize) -> f64 {
    let t0 = Instant::now();
    for i in 0..n {
        let ip: std::net::Ipv4Addr = format!(
            "{}.{}.{}.{}",
            (i >> 24) & 0xFF,
            (i >> 16) & 0xFF,
            (i >> 8) & 0xFF,
            i & 0xFF
        )
        .parse()
        .unwrap();
        std::hint::black_box(ramshield_storage::subnet_key_v4(ip.octets()));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_subnet_key_v6(n: usize) -> f64 {
    let t0 = Instant::now();
    for i in 0..n {
        let mut octets = [0u8; 16];
        octets[0] = 0x20;
        octets[1] = 0x01;
        octets[2] = 0x0d;
        octets[3] = 0xb8;
        octets[8] = (i >> 8) as u8;
        octets[9] = i as u8;
        std::hint::black_box(ramshield_storage::subnet_key_v6(octets));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn default_record() -> ramshield_storage::IpRecord {
    ramshield_storage::IpRecord {
        ip: "0.0.0.0".parse().unwrap(),
        request_count: 0,
        ewma_rps: 0.0,
        cusum_s: 0.0,
        baseline_rps: 0.0,
        prev_sample_hot: false,
        sample_count: 0,
        pulse_samples_in_window: 0,
        pulse_window_start_ns: 0,
        first_seen_ns: 0,
        last_seen_ns: 0,
        bytes_in: 0,
        status_dist: [0; 5],
        proto_fingerprint: 0,
        threat_score: 0.0,
        block_state: ramshield_storage::BlockState::Clean,
    }
}

fn make_ip(i: usize) -> IpAddr {
    format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
        .parse()
        .unwrap()
}

fn bench_store_get(n: usize) -> f64 {
    let store = ramshield_storage::Store::new(64);
    let limit = 1024 * 1024 * 1024;
    for i in 0..1000 {
        let ip = make_ip(i);
        let mut def = default_record();
        def.ip = ip;
        def.request_count = 100;
        store.update_ip(ip, def, limit, |_| {});
    }
    let t0 = Instant::now();
    for i in 0..n {
        let ip = make_ip(i % 1000);
        std::hint::black_box(store.get(&ip));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_store_update_ip(n: usize) -> f64 {
    let store = ramshield_storage::Store::new(64);
    let limit = 1024 * 1024 * 1024;
    for i in 0..1000 {
        let ip = make_ip(i);
        let mut def = default_record();
        def.ip = ip;
        store.update_ip(ip, def, limit, |_| {});
    }
    let t0 = Instant::now();
    for i in 0..n {
        let ip = make_ip(i % 1000);
        store.update_ip(ip, default_record(), limit, |rec| {
            rec.request_count += 1;
            rec.ewma_rps =
                ramshield_detection::rate_tracker::ewma(rec.ewma_rps, rec.request_count as f64);
            rec.bytes_in += 256;
            rec.last_seen_ns = 1_000_000_000 + i as u64 * 1_000_000;
        });
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

// ── Protocol ─────────────────────────────────────────────────────────────────

fn bench_hmac_sign(n: usize) -> f64 {
    let key = vec![0xABu8; 32];
    let payload = b"{\"type\":\"report\",\"data\":\"test\"}";
    let t0 = Instant::now();
    for i in 0..n {
        std::hint::black_box(ramshield_protocol::auth::sign(&key, i as u64, payload));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_request_serialize(n: usize) -> f64 {
    use ramshield_protocol::message::{ConnectionReport, Request};
    let req = Request::ReportConnections {
        events: (0..100)
            .map(|i| ConnectionReport {
                ip: format!("10.0.{}.{}", (i >> 8) & 0xFF, i & 0xFF)
                    .parse()
                    .unwrap(),
                bytes: 256,
                status_code: 200,
                proto_fp: 1,
            })
            .collect(),
    };
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(serde_json::to_vec(&req).unwrap());
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_request_deserialize(n: usize) -> f64 {
    use ramshield_protocol::message::{ConnectionReport, Request};
    let req = Request::ReportConnections {
        events: (0..100)
            .map(|i| ConnectionReport {
                ip: format!("10.0.{}.{}", (i >> 8) & 0xFF, i & 0xFF)
                    .parse()
                    .unwrap(),
                bytes: 256,
                status_code: 200,
                proto_fp: 1,
            })
            .collect(),
    };
    let json = serde_json::to_vec(&req).unwrap();
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(serde_json::from_slice::<Request>(&json).unwrap());
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

// ── Metrics ──────────────────────────────────────────────────────────────────

fn bench_inc_requests(n: usize) -> f64 {
    let m = ramshield_metrics::Metrics::new();
    let t0 = Instant::now();
    for _ in 0..n {
        m.inc_requests();
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_inc_ingested(n: usize) -> f64 {
    let m = ramshield_metrics::Metrics::new();
    let t0 = Instant::now();
    for _ in 0..n {
        m.inc_ingested(100);
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn bench_render_prometheus(n: usize) -> f64 {
    let m = ramshield_metrics::Metrics::new();
    for _ in 0..1000 {
        m.inc_requests();
        m.inc_ingested(50);
    }
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(m.render_prometheus());
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let n_fast = 100_000;
    let n_medium = 10_000;
    let n_slow = 1_000;

    println!("═══════════════════════════════════════════════════════════════════════════");
    println!("  RAMSHIELD HOT-PATH BENCHMARKS  (ns/op lower = faster)");
    println!("═══════════════════════════════════════════════════════════════════════════");

    println!("\n── Forecasting ──────────────────────────────────────────────────────");
    bench!("HoltWinters::update", bench_hw_update, n_fast);
    bench!(
        "HypothesisTracker::bayesian_update",
        bench_bayesian_update,
        n_medium
    );
    bench!(
        "HypothesisTracker::best_above_threshold",
        bench_bayesian_best_above,
        n_fast
    );

    println!("\n── Detection ───────────────────────────────────────────────────────");
    bench!("ewma()", bench_ewma, n_fast);
    bench!("cusum_step_capped + cusum_fired", bench_cusum, n_fast);
    bench!("pulse_tracker_step", bench_pulse, n_fast);
    bench!(
        "BloomFilter::insert (8M bits)",
        bench_bloom_insert,
        n_medium
    );
    bench!(
        "BloomFilter::contains (8M bits)",
        bench_bloom_contains,
        n_medium
    );
    bench!(
        "batch::aggregate (4096 events)",
        bench_batch_aggregate,
        n_slow
    );

    println!("\n── Storage ─────────────────────────────────────────────────────────");
    bench!("subnet_key_v4", bench_subnet_key_v4, n_fast);
    bench!("subnet_key_v6", bench_subnet_key_v6, n_fast);
    bench!("Store::get (1K entries)", bench_store_get, n_medium);
    bench!(
        "Store::update_ip (1K entries)",
        bench_store_update_ip,
        n_medium
    );

    println!("\n── Protocol ────────────────────────────────────────────────────────");
    bench!("auth::sign HMAC-SHA256", bench_hmac_sign, n_medium);
    bench!(
        "Request::serialize (100 events)",
        bench_request_serialize,
        n_medium
    );
    bench!(
        "Request::deserialize (100 events)",
        bench_request_deserialize,
        n_medium
    );

    println!("\n── Metrics ─────────────────────────────────────────────────────────");
    bench!("inc_requests (AtomicU64)", bench_inc_requests, n_fast);
    bench!("inc_ingested (AtomicU64)", bench_inc_ingested, n_fast);
    bench!(
        "render_prometheus (text format)",
        bench_render_prometheus,
        n_medium
    );

    println!("\n═══════════════════════════════════════════════════════════════════════════");
}
