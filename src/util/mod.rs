use std::collections::{HashMap, VecDeque};
use serde::{Deserialize, Serialize};

/// Bounded VecDeque for fixed-size history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedVecDeque<T> {
    buf: VecDeque<T>,
    cap: usize,
}

impl<T> BoundedVecDeque<T> {
    pub fn new(cap: usize) -> Self {
        Self { buf: VecDeque::with_capacity(cap), cap }
    }

    pub fn push(&mut self, v: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.buf.iter()
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn front(&self) -> Option<&T> {
        self.buf.front()
    }

    pub fn back(&self) -> Option<&T> {
        self.buf.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_vec_deque_keeps_capacity() {
        let mut b = BoundedVecDeque::new(3);
        b.push(1);
        b.push(2);
        b.push(3);
        assert_eq!(b.len(), 3);
        b.push(4); // should evict 1
        assert_eq!(b.len(), 3);
        assert_eq!(*b.front().unwrap(), 2);
        assert_eq!(*b.back().unwrap(), 4);
    }

    #[test]
    fn bounded_vec_deque_empty() {
        let b: BoundedVecDeque<i32> = BoundedVecDeque::new(5);
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn bounded_vec_deque_capacity_one() {
        let mut b = BoundedVecDeque::new(1);
        b.push(10);
        assert_eq!(b.len(), 1);
        b.push(20);
        assert_eq!(b.len(), 1);
        assert_eq!(*b.front().unwrap(), 20);
    }
}

/// Data processing utilities for RamShield
pub struct DataProcessor;

impl DataProcessor {
    /// Convert IP address to feature vector
    pub fn ip_to_features(ip_str: &str) -> Vec<f64> {
        // Convert IP string to numerical features
        let parts: Vec<&str> = ip_str.split('.').collect();
        if parts.len() != 4 {
            return vec![0.0; 4];
        }

        parts.iter().filter_map(|&part| {
            part.parse::<f64>().ok()
        }).collect()
    }

    /// Calculate entropy of data
    pub fn calculate_entropy(data: &[u8]) -> f64 {
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

    /// Extract features from connection data
    pub fn extract_features(data: &[u8]) -> HashMap<String, f64> {
        let mut features = HashMap::new();

        // Basic statistical features
        features.insert("length".to_string(), data.len() as f64);
        features.insert("entropy".to_string(), Self::calculate_entropy(data));

        // Byte distribution features
        if !data.is_empty() {
            let sum: u64 = data.iter().map(|&x| x as u64).sum();
            features.insert("byte_sum".to_string(), sum as f64);

            let avg = sum as f64 / data.len() as f64;
            features.insert("byte_avg".to_string(), avg);
        }

        features
    }
}