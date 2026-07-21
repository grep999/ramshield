use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

pub struct DnsForecaster {
    /// DNS query patterns and their threat scores
    patterns: Arc<RwLock<HashMap<String, DnsPattern>>>,
}

#[derive(Debug, Clone)]
pub struct DnsQueryRecord {
    pub domain: String,
    pub response_ips: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DnsPattern {
    pub pattern: String,
    pub frequency: u64,
    pub threat_score: f32,
    pub last_seen: SystemTime,
    pub associated_ips: Vec<String>,
}

impl DnsForecaster {
    pub fn new() -> Self {
        Self {
            patterns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a DNS query and update patterns
    pub async fn record_query(&self, record: DnsQueryRecord) {
        // Update pattern analysis
        self.update_pattern_analysis(&record).await;
    }

    /// Update pattern analysis based on query
    async fn update_pattern_analysis(&self, record: &DnsQueryRecord) {
        // Extract pattern from domain
        let pattern = self.extract_pattern(&record.domain);

        // Update pattern frequency and threat score
        {
            let mut patterns = self.patterns.write().await;
            let entry = patterns
                .entry(pattern.clone())
                .or_insert_with(|| DnsPattern {
                    pattern: pattern.clone(),
                    frequency: 0,
                    threat_score: 0.0,
                    last_seen: SystemTime::now(),
                    associated_ips: Vec::new(),
                });

            entry.frequency += 1;
            entry.last_seen = SystemTime::now();

            // Add unique IPs to associated IPs
            for ip in &record.response_ips {
                if !entry.associated_ips.contains(ip) {
                    entry.associated_ips.push(ip.clone());
                }
            }

            // Update threat score based on pattern characteristics
            entry.threat_score = self.calculate_threat_score(entry, record);
        }
    }

    /// Extract pattern from domain name
    fn extract_pattern(&self, domain: &str) -> String {
        // Suspicious patterns
        if domain.contains("bot") || domain.contains("scan") || domain.contains("exploit") {
            return "suspicious".to_string();
        }

        // CDN/Cloud patterns
        if domain.contains("cloud") || domain.contains("cdn") || domain.contains("aws") {
            return "cloud".to_string();
        }

        // Generic patterns
        if domain.contains("com") || domain.contains("net") || domain.contains("org") {
            return "generic".to_string();
        }

        "unknown".to_string()
    }

    /// Calculate threat score for a DNS pattern
    fn calculate_threat_score(&self, pattern: &DnsPattern, _record: &DnsQueryRecord) -> f32 {
        let mut score: f32 = 0.0;

        // High frequency increases threat score
        if pattern.frequency > 100 {
            score += 0.3;
        }

        // Suspicious pattern type
        if pattern.pattern == "suspicious" {
            score += 0.5;
        }

        // Many associated IPs increases threat score
        if pattern.associated_ips.len() > 10 {
            score += 0.2;
        }

        // Recent activity increases threat score
        if let Ok(elapsed) = pattern.last_seen.elapsed() {
            if elapsed < Duration::from_secs(300) {
                score += 0.1;
            }
        }

        // Cap at 1.0 and return
        score.min(1.0)
    }

    /// Get threat score for a domain
    pub async fn get_threat_score(&self, domain: &str) -> f32 {
        let pattern_type = self.extract_pattern(domain);
        let patterns = self.patterns.read().await;

        if let Some(pattern) = patterns.get(&pattern_type) {
            pattern.threat_score
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_dns_forecaster() {
        let forecaster = DnsForecaster::new();

        let record = DnsQueryRecord {
            domain: "suspicious-botnet.com".to_string(),
            response_ips: vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()],
        };

        forecaster.record_query(record).await;
    }
}
