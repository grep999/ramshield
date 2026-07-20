use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

pub mod xgboost;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackPattern {
    pub id: String,
    pub signature: Vec<u8>,
    pub frequency: u64,
    pub threat_level: f32,
    pub last_seen: std::time::SystemTime,
    /// Pattern metadata for analysis
    pub metadata: PatternMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMetadata {
    pub first_seen: SystemTime,
    pub last_seen: SystemTime,
    pub source_ips: Vec<String>,
    pub byte_patterns: HashMap<u8, u32>,
    pub request_rates: Vec<f64>,
    pub size: usize,
    pub entropy_history: Vec<f64>,
}

impl Default for PatternMetadata {
    fn default() -> Self {
        Self {
            first_seen: SystemTime::UNIX_EPOCH,
            last_seen: SystemTime::UNIX_EPOCH,
            source_ips: Vec::new(),
            byte_patterns: HashMap::new(),
            request_rates: Vec::new(),
            size: 0,
            entropy_history: Vec::new(),
        }
    }
}

pub struct PatternLearner {
    patterns: Arc<RwLock<HashMap<String, AttackPattern>>>,
    /// Threshold for pattern recognition (0.0 - 1.0)
    similarity_threshold: f32,
}

impl PatternLearner {
    pub fn new(similarity_threshold: f32) -> Self {
        Self {
            patterns: Arc::new(RwLock::new(HashMap::new())),
            similarity_threshold,
        }
    }

    /// Learn a new pattern from incoming data and track micro-attack patterns
    pub fn learn_pattern(&self, data: &[u8], threat_level: f32, source_ip: &str) -> String {
        let pattern_id = self.generate_pattern_id(data);
        let mut patterns = self.patterns.write().unwrap();

        let pattern = patterns.entry(pattern_id.clone()).or_insert_with(|| {
            AttackPattern {
                id: pattern_id.clone(),
                signature: data.to_vec(),
                frequency: 0,
                threat_level: 0.0,
                last_seen: std::time::SystemTime::now(),
                metadata: PatternMetadata::default(),
            }
        });

        pattern.frequency += 1;
        pattern.threat_level = (pattern.threat_level + threat_level) / 2.0;
        pattern.last_seen = std::time::SystemTime::now();

        // Update metadata
        pattern.metadata.size = data.len();
        pattern.metadata.last_seen = std::time::SystemTime::now();

        // Track source IPs
        if !pattern.metadata.source_ips.contains(&source_ip.to_string()) {
            pattern.metadata.source_ips.push(source_ip.to_string());
        }

        // Track byte patterns for better analysis
        for &byte in data {
            *pattern.metadata.byte_patterns.entry(byte).or_insert(0) += 1;
        }

        // Calculate and store entropy
        let entropy = self.calculate_entropy(data);
        pattern.metadata.entropy_history.push(entropy);

        pattern_id
    }

    /// Detect if data matches a known pattern with enhanced micro-attack detection
    pub fn detect_pattern(&self, data: &[u8]) -> Option<AttackPattern> {
        let patterns = self.patterns.read().unwrap();
        for (_, pattern) in patterns.iter() {
            if self.calculate_similarity(data, &pattern.signature) > self.similarity_threshold {
                return Some(pattern.clone());
            }
        }
        None
    }

    /// Calculate entropy of data
    fn calculate_entropy(&self, data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut frequency = [0u32; 256];
        for &byte in data {
            frequency[byte as usize] += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;

        for &freq in &frequency {
            if freq > 0 {
                let prob = freq as f64 / len;
                entropy -= prob * prob.log2();
            }
        }

        entropy
    }

    /// Get all known patterns
    pub fn get_patterns(&self) -> Vec<AttackPattern> {
        let patterns = self.patterns.read().unwrap();
        patterns.values().cloned().collect()
    }

    /// Generate a unique ID for a pattern
    fn generate_pattern_id(&self, data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Calculate similarity between two data sets (0.0 - 1.0)
    fn calculate_similarity(&self, a: &[u8], b: &[u8]) -> f32 {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }

        // Simple similarity calculation based on common elements
        let common_count = a.iter().filter(|&x| b.contains(x)).count();
        let max_len = a.len().max(b.len());

        if max_len == 0 {
            0.0
        } else {
            common_count as f32 / max_len as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_learning() {
        let learner = PatternLearner::new(0.8);
        let data = b"test_pattern_data";

        let pattern_id = learner.learn_pattern(data, 0.9, "127.0.0.1");
        assert!(!pattern_id.is_empty());

        let patterns = learner.get_patterns();
        assert!(!patterns.is_empty());
        assert_eq!(patterns[0].frequency, 1);
    }

    #[test]
    fn test_pattern_detection() {
        let learner = PatternLearner::new(0.8);
        let data = b"test_pattern_data";

        // Learn a pattern
        learner.learn_pattern(data, 0.9, "127.0.0.1");

        // Detect similar pattern
        let similar_data = b"test_pattern_data_modified";
        let detected = learner.detect_pattern(similar_data);
        assert!(detected.is_some());
    }
}