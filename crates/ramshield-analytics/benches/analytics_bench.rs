/// A-037 placeholder benchmarks that compile under core-only all-targets.
/// Real measurement of analytics/storage paths is tracked separately under A-037.
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn bench_noop_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("noop_insert");
    group.bench_function("insert", |b| {
        b.iter(|| {
            black_box(42u64);
        });
    });
    group.finish();
}

fn bench_noop_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("noop_append");
    group.bench_function("append", |b| {
        b.iter(|| {
            black_box("test");
        });
    });
    group.finish();
}

fn bench_noop_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("noop_set");
    group.bench_function("set", |b| {
        b.iter(|| {
            black_box(24u8);
        });
    });
    group.finish();
}

criterion_group!(
    analytics_benches,
    bench_noop_insert,
    bench_noop_append,
    bench_noop_set
);
criterion_main!(analytics_benches);
