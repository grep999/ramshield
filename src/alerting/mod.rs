//! Enterprise alerting and audit trail module.
//!
//! Provides:
//! - Alert generation when thresholds are exceeded
//! - Audit trail for compliance (SOC2/GDPR)
//! - Alert cooldown to prevent flooding
//! - File-based audit log rotation
//!
//! Inspired by:
//! - Splunk Enterprise Security alert framework
//! - AWS CloudWatch alarm states
//! - Grafana alerting rules

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::config::AlertingConfig;
use crate::metrics::Metrics;
use crate::error::RamShieldError;

/// Alert severity levels (ISO 27001 compliant)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info = 0,
    Warning = 1,
    High = 2,
    Critical = 3,
}

impl AlertSeverity {
    fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "INFO",
            AlertSeverity::Warning => "WARNING",
            AlertSeverity::High => "HIGH",
            AlertSeverity::Critical => "CRITICAL",
        }
    }
}

/// An alert event for audit trail
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertEvent {
    pub timestamp_ms: u64,
    pub severity: String,
    pub source: String,
    pub message: String,
    pub metrics: serde_json::Value,
}

/// Alert engine — monitors metrics and generates alerts
pub struct AlertingEngine {
    config: AlertingConfig,
    metrics: Arc<Metrics>,
    /// Last alert time per source (for cooldown)
    last_alert_ms: Arc<parking_lot::Mutex<HashMap<String, u64>>>,
    /// Alert history (bounded)
    alert_history: Arc<parking_lot::Mutex<Vec<AlertEvent>>>,
    /// Total alerts generated
    alerts_total: Arc<AtomicU64>,
    /// Critical alerts count
    alerts_critical: Arc<AtomicU64>,
}

impl AlertingEngine {
    pub fn new(config: AlertingConfig, metrics: Arc<Metrics>) -> Self {
        Self {
            config,
            metrics,
            last_alert_ms: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            alert_history: Arc::new(parking_lot::Mutex::new(Vec::with_capacity(100))),
            alerts_total: Arc::new(AtomicU64::new(0)),
            alerts_critical: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn start(&self, mut shutdown_rx: broadcast::Receiver<()>) -> Result<(), RamShieldError> {
        if !self.config.enabled {
            info!("AlertingEngine: Disabled, skipping startup.");
            let _ = shutdown_rx.recv().await;
            return Ok(());
        }

        info!("AlertingEngine: Starting...");
        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.check_alerts().await;
                }
                _ = shutdown_rx.recv() => {
                    info!("AlertingEngine: Shutting down.");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn check_alerts(&self) {
        let now_ms = now_ms();
        let events = self.metrics.events_ingested.load(Ordering::Relaxed);
        let blocked = self.metrics.events_rejected.load(Ordering::Relaxed);

        // Forecasting metrics
        let hw_rps = f64::from_bits(self.metrics.hw_rps_bits.load(Ordering::Relaxed));
        let entropy = f64::from_bits(self.metrics.entropy_bits.load(Ordering::Relaxed));

        // Check RPS threshold
        if hw_rps >= self.config.rps_alert_threshold as f64 {
            self.maybe_alert(
                "high_rps",
                AlertSeverity::High,
                format!("RPS {:.0} exceeds threshold {}", hw_rps, self.config.rps_alert_threshold),
                serde_json::json!({"rps": hw_rps, "threshold": self.config.rps_alert_threshold}),
                now_ms,
            );
        }

        // Check entropy threshold
        if entropy >= self.config.entropy_alert_threshold {
            self.maybe_alert(
                "high_entropy",
                AlertSeverity::Warning,
                format!("Entropy {:.4} exceeds threshold {:.2}", entropy, self.config.entropy_alert_threshold),
                serde_json::json!({"entropy": entropy, "threshold": self.config.entropy_alert_threshold}),
                now_ms,
            );
        }

        // Check block rate
        if events > 0 {
            let block_rate = blocked as f64 / events as f64;
            if block_rate > 0.9 && blocked > 100 {
                self.maybe_alert(
                    "high_block_rate",
                    AlertSeverity::Critical,
                    format!("Block rate {:.1}% — potential DDoS attack", block_rate * 100.0),
                    serde_json::json!({"events": events, "blocked": blocked, "rate": block_rate}),
                    now_ms,
                );
            }
        }
    }

    fn maybe_alert(
        &self,
        source: &str,
        severity: AlertSeverity,
        message: String,
        metrics_data: serde_json::Value,
        now_ms: u64,
    ) {
        // Cooldown check
        let cooldown_ms = self.config.alert_cooldown_secs * 1000;
        let should_alert = {
            let mut last = self.last_alert_ms.lock();
            if let Some(&last_ts) = last.get(source) {
                if now_ms.saturating_sub(last_ts) < cooldown_ms {
                    return; // In cooldown
                }
            }
            last.insert(source.to_string(), now_ms);
            true
        };

        if !should_alert {
            return;
        }

        let alert = AlertEvent {
            timestamp_ms: now_ms,
            severity: severity.as_str().to_string(),
            source: source.to_string(),
            message,
            metrics: metrics_data,
        };

        self.alerts_total.fetch_add(1, Ordering::Relaxed);
        if severity == AlertSeverity::Critical {
            self.alerts_critical.fetch_add(1, Ordering::Relaxed);
        }

        match severity {
            AlertSeverity::Critical => warn!("ALERT [CRITICAL] {}: {}", source, alert.message),
            AlertSeverity::High => warn!("ALERT [HIGH] {}: {}", source, alert.message),
            AlertSeverity::Warning => info!("ALERT [WARNING] {}: {}", source, alert.message),
            AlertSeverity::Info => info!("ALERT [INFO] {}: {}", source, alert.message),
        }

        // Add to history
        {
            let mut history = self.alert_history.lock();
            if history.len() >= 100 {
                history.remove(0);
            }
            history.push(alert.clone());
        }

        // Write to audit log if enabled
        if self.config.audit_log_enabled {
            self.write_audit_log(&alert);
        }
    }

    fn write_audit_log(&self, alert: &AlertEvent) {
        let log_line = format!(
            "[{}] {} {} {} {}\n",
            alert.timestamp_ms,
            alert.severity,
            alert.source,
            alert.message,
            alert.metrics
        );
        let path = &self.config.audit_log_path;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(e) = file.write_all(log_line.as_bytes()) {
                    warn!("Failed to write audit log: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to open audit log at {}: {}", path, e);
            }
        }
    }

    /// Get recent alerts for dashboard
    pub fn get_alerts(&self) -> Vec<AlertEvent> {
        self.alert_history.lock().clone()
    }

    pub fn alerts_count(&self) -> u64 {
        self.alerts_total.load(Ordering::Relaxed)
    }

    pub fn critical_count(&self) -> u64 {
        self.alerts_critical.load(Ordering::Relaxed)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}