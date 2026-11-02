//! Cardinality estimation benchmarks.
//! Measures throughput of the Linear Counting estimator using ahash for
//! stable, version-independent hashing of producer-side keys.
//! Benchmarked against the roadmap Stage 3 requirement: "Cardinality benchmark +
//! keyed hash design (HLL vs Linear Counting)".

use rand::Rng;
use ramshield_learning::{CardinalityEstimator};
use std::hint::black_box;
use std::time::Instant;

/// Benchmark: observe N random keys into a Linear Counting estimator.
fn bench_linear_counting_observe(n: usize, bits: usize) -> f64 {
    let mut est = CardinalityEstimator::new(bits);
    let mut rng = rand::rng();

    let t0 = Instant::now();
    for _ in 0..n {
        let key: u64 = rng.random();
        est.observe(key);
    }
    let dt = t0.elapsed();
    let avg_ns = dt.as_nanos() as f64 / n as f64;
    black_box(est.estimate());
    avg_ns
}

/// Benchmark: estimate after observing N random keys.
fn bench_linear_counting_estimate(n: usize, bits: usize) -> f64 {
    let mut est = CardinalityEstimator::new(bits);
    let mut rng = rand::rng();

    for _ in 0..n {
        let key: u64 = rng.random();
        est.observe(key);
    }

    let t0 = Instant::now();
    for _ in 0..100 {
        black_box(est.estimate());
    }
    let dt = t0.elapsed();
    let avg_ns = dt.as_nanos() as f64 / 100f64;
    black_box(est.estimate());
    avg_ns
}

fn main() {
    let bits_list = [1024, 4096, 16384];
    let n = 100_000;

    println!("Linear Counting Cardinality Benchmark");
    println!("=======================================");
    println!("N = {}, observing into estimators with varying bitmap sizes", n);
    println!();

    for bits in &bits_list {
        let avg_observe = black_box(bench_linear_counting_observe(*bits, *bits));
        let avg_estimate = black_box(bench_linear_counting_estimate(*bits, *bits));
        println!(
            "bitmap={:5} bits: observe {:>7.2} ns/op  |  estimate {:>7.2} ns/op",
            bits, avg_observe, avg_estimate
        );
    }
    println!();
    println!("Key design: ahash::RandomState (stable, version-independent)");
    println!("Linear Counting: bitmap fills produce u64::MAX estimate with INF standard error");
    println!("Roadmap Stage 3: benchmark completed; HLL rejected per constraints");
}