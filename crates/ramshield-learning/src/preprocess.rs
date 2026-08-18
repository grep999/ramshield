//! Preprocessing module stub (experimental only).

/// Identity normalization stub.
#[cfg(feature = "experimental-ml")]
pub fn normalize(data: &[f64]) -> Vec<f64> {
    data.to_vec()
}

#[cfg(all(test, feature = "experimental-ml"))]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_identity() {
        let input = vec![1.0, 2.0];
        assert_eq!(normalize(&input), input);
    }
}
