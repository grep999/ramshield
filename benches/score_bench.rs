use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ramshield::learning::xgboost::score;

fn bench_score(c: &mut Criterion) {
    c.bench_function("xgboost::score", |b| b.iter(|| black_box(score())));
}

criterion_group!(benches, bench_score);
criterion_main!(benches);
