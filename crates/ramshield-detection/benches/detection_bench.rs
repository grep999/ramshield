/// A-037 Core benchmarks and regression budgets for ramshield-detection.
/// Establishes reproducible baselines for detection operations under load.
use criterion::{Criterion, criterion_group, criterion_main};
use ramshield_detection::rate_tracker::PulseTracker;

/// Benchmark rate tracker pulse observation - measures throughput
fn bench_rate_tracker_pulse(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_tracker_pulse");
    group.bench_function("observe", |b| {
        b.iter(|| {
            let mut tracker = PulseTracker::new();
            let _ = tracker.observe(1_000_000_000, 150);
        });
    });
    group.finish();
}

/// Benchmark rate tracker can_emit - measures pulse decision throughput
fn bench_rate_tracker_can_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_tracker_can_emit");
    group.bench_function("can_emit_pulse", |b| {
        b.iter(|| {
            let tracker = PulseTracker::new();
            let _ = tracker.can_emit_pulse(1_000_000_000);
        });
    });
    group.finish();
}

criterion_group!(
    detection_benches,
    bench_rate_tracker_pulse,
    bench_rate_tracker_can_emit
);
criterion_main!(detection_benches);
