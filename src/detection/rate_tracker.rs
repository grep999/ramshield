pub const ALPHA: f64 = 0.3;

pub fn ewma(prev: f64, sample: f64) -> f64 {
    ALPHA * sample + (1.0 - ALPHA) * prev
}

pub fn is_exceeded(ewma_rps: f64, threshold: u64) -> bool {
    ewma_rps > threshold as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges() {
        let mut e = 0.0f64;
        for _ in 0..200 { e = ewma(e, 500.0); }
        assert!((e - 500.0).abs() < 0.1, "ewma={}", e);
    }

    #[test]
    fn spike_dampened() {
        let mut e = 0.0f64;
        for _ in 0..20 { e = ewma(e, 100.0); }
        e = ewma(e, 50_000.0);
        assert!(e < 16_000.0, "ewma={}", e);
    }

    #[test]
    fn threshold() {
        assert!(is_exceeded(1001.0, 1000));
        assert!(!is_exceeded(999.9, 1000));
    }
}
