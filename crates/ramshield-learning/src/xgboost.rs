//! XGBoost scoring module stub (experimental only).

/// Stub XGBoost score.
///
/// Returns `0.0` because no trained model is present.
#[inline]
#[cfg(feature = "experimental-ml")]
pub fn score() -> f64 {
    0.0
}

#[cfg(all(test, feature = "experimental-ml"))]
mod tests {
    use super::*;

    #[test]
    fn test_score_stub() {
        assert_eq!(score(), 0.0);
    }
}
