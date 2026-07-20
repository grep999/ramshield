//! Preprocessing module stub.

/// Normalizes input data.
pub fn normalize(data: &[f64]) -> Vec<f64> {
    data.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        let input = vec![1.0, 2.0];
        assert_eq!(normalize(&input), input);
    }
}
