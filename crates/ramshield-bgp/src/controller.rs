use crate::config::BgpConfig;
use crate::rtbh::announce_rtbh;
use crate::{BgpError, Result};

/// Schmitt trigger controller — debounces rapid state transitions
///
/// Engages the RTBH filter when BGP session flaps exceed the engage threshold,
/// and disengages only after the disengage threshold has elapsed without flaps.
/// This prevents filtering from a single transient event.
#[derive(Debug, Clone)]
pub struct SchmittTrigger {
    /// Engage threshold: flaps within this window trigger RTBH activation
    engage_threshold: u16,
    /// Disengage threshold: elapsed flaps without activity trigger RTBH deactivation
    disengage_threshold: u16,
    /// Flap counter within the current window
    flap_count: u16,
    /// Timer handle (would be tokio::time::Instant in async impl)
    engaged_at: Option<std::time::Instant>,
    /// Timer handle for disengage
    disengage_timer: Option<std::time::Instant>,
}

impl SchmittTrigger {
    pub fn new(engage_secs: u16, disengage_secs: u16) -> Self {
        SchmittTrigger {
            engage_threshold: engage_secs,
            disengage_threshold: disengage_secs,
            flap_count: 0,
            engaged_at: None,
            disengage_timer: None,
        }
    }

    /// Record a BGP session flap
    pub fn record_flap(&mut self) {
        self.flap_count += 1;
    }

    /// Check if RTBH should be engaged
    pub fn should_engage(&self) -> bool {
        self.flap_count >= self.engage_threshold
    }

    /// Check if RTBH should be disengaged
    pub fn should_disengage(&self) -> bool {
        self.flap_count < self.disengage_threshold
            && self.engaged_at.map_or(true, |t| {
                t.elapsed().as_secs() >= (self.disengage_threshold as u64)
            })
    }

    /// Reset flap counter
    pub fn reset(&mut self) {
        self.flap_count = 0;
        self.engaged_at = None;
        self.disengage_timer = None;
    }
}

impl Default for SchmittTrigger {
    fn default() -> Self {
        Self::new(5, 15)
    }
}

/// Apply the Schmitt trigger's decision to the BGP config / session
///
/// If the trigger says engage, return the RTBH announce message bytes.
/// If it says disengage, return None (clear the filter).
pub fn apply_trigger(
    config: &mut BgpConfig,
    trigger: &mut SchmittTrigger,
) -> Option<Result<Vec<u8>>> {
    if trigger.should_engage() {
        let peer_ip_str = match config.peer_ip {
            std::net::IpAddr::V4(v4) => format!("{}.0.0.0", v4.octets()[0]),
            std::net::IpAddr::V6(_) => "198.51.100.0".to_string(),
        };
        Some(announce_rtbh(&peer_ip_str).map(|(bh, _)| bh))
    } else if trigger.should_disengage() {
        trigger.reset();
        None
    } else {
        None
    }
}
