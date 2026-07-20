//! Preprocessing module stub.

/// Normalizes input data.
pub fn normalize(data: &[f64]) -> Vec<f64> {
    data.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_normalize() {
        let input = vec![1.0, 2.0];
        assert_eq!(normalize(&input), input);
    }

    proptest! {
        fn prop_normalize_preserves_length(data in prop::collection::vec(-1000.0..1000.0, 0..100)) {
            let result = normalize(&data);
            prop_assert_eq!(result.len(), data.len());
        }

        fn prop_normalize_preserves_values(data in prop::collection::vec(-1000.0..1000.0, 0..100)) {
            let result = normalize(&data);
            for (i, &v) in data.iter().enumerate() {
                prop_assert_eq!(result[i], v);
            }
        }

        fn prop_normalize_empty_vec(_ in proptest::collection::vec(-1000.0..1000.0, 0..0)) {
            let result = normalize(&[]);
            prop_assert!(result.is_empty());
        }
    }
}
