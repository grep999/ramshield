use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Feature vector for threat scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatFeatures {
    /// Byte entropy of payload.
    pub entropy: f64,
    /// Requests per second from source.
    pub request_rate: f64,
    /// Payload size in bytes.
    pub payload_size: usize,
    /// Unique byte ratio (unique/total).
    pub unique_byte_ratio: f64,
    /// Number of distinct source IPs.
    pub source_diversity: usize,
    /// Time since first observation (seconds).
    pub observation_age: f64,
}

impl ThreatFeatures {
    /// Convert to dense feature vector for model input.
    pub fn to_vec(&self) -> Vec<f32> {
        vec![
            self.entropy as f32,
            self.request_rate as f32,
            self.payload_size as f32,
            self.unique_byte_ratio as f32,
            self.source_diversity as f32,
            self.observation_age as f32,
        ]
    }
}

/// XGBoost-based threat scorer.
pub struct XGBoostScorer {
    /// Learned weights per feature (placeholder for real model).
    weights: Vec<f32>,
    /// Bias term.
    bias: f32,
    /// Confidence threshold for blocking.
    block_threshold: f32,
}

impl XGBoostScorer {
    pub fn new(block_threshold: f32) -> Self {
        let num_features = 6;
        Self {
            weights: vec![0.0; num_features],
            bias: 0.0,
            block_threshold,
        }
    }

    /// Score a feature vector. Returns (score, should_block).
    pub fn score(&self, features: &ThreatFeatures) -> (f32, bool) {
        let vec = features.to_vec();
        let raw: f32 = vec
            .iter()
            .zip(self.weights.iter())
            .map(|(x, w)| x * w)
            .sum::<f32>()
            + self.bias;
        let score = sigmoid(raw);
        (score, score >= self.block_threshold)
    }

    /// Update weights with a single labeled sample (online learning stub).
    pub fn train_step(&mut self, features: &ThreatFeatures, label: f32, lr: f32) {
        let vec = features.to_vec();
        let pred = sigmoid(
            vec.iter()
                .zip(self.weights.iter())
                .map(|(x, w)| x * w)
                .sum::<f32>()
                + self.bias,
        );
        let error = pred - label;
        for (w, x) in self.weights.iter_mut().zip(vec.iter()) {
            *w -= lr * error * x;
        }
        self.bias -= lr * error;
    }

    pub fn weights(&self) -> &[f32] {
        &self.weights
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_returns_float() {
        let scorer = XGBoostScorer::new(0.5);
        let features = ThreatFeatures {
            entropy: 6.5,
            request_rate: 100.0,
            payload_size: 1024,
            unique_byte_ratio: 0.3,
            source_diversity: 5,
            observation_age: 30.0,
        };
        let (score, _) = scorer.score(&features);
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn test_train_step_updates_weights() {
        let mut scorer = XGBoostScorer::new(0.5);
        let features = ThreatFeatures {
            entropy: 7.0,
            request_rate: 200.0,
            payload_size: 2048,
            unique_byte_ratio: 0.5,
            source_diversity: 10,
            observation_age: 60.0,
        };
        let old_weights = scorer.weights().to_vec();
        scorer.train_step(&features, 1.0, 0.01);
        assert_ne!(scorer.weights(), old_weights.as_slice());
    }
}
