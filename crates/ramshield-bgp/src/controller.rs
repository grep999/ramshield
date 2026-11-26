use crate::Result;
use crate::config::BgpConfig;
use crate::rtbh::announce_rtbh;

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
    pub engaged_at: Option<std::time::Instant>,
    /// Timer handle for disengage
    pub disengage_timer: Option<std::time::Instant>,
    /// Most recent flap; disengage requires a full quiet window after this
    pub last_flap_at: Option<std::time::Instant>,
}

impl SchmittTrigger {
    pub fn new(engage_secs: u16, disengage_secs: u16) -> Self {
        SchmittTrigger {
            engage_threshold: engage_secs,
            disengage_threshold: disengage_secs,
            flap_count: 0,
            engaged_at: None,
            disengage_timer: None,
            last_flap_at: None,
        }
    }

    /// Record a BGP session flap
    pub fn record_flap(&mut self) {
        // ponytail: saturate; 65k flaps must not wrap to 0 and re-arm
        self.flap_count = self.flap_count.saturating_add(1);
        self.last_flap_at = Some(std::time::Instant::now());
    }

    /// Check if RTBH should be engaged
    pub fn should_engage(&self) -> bool {
        self.engaged_at.is_none() && self.flap_count >= self.engage_threshold
    }

    /// Check if RTBH should be disengaged
    pub fn should_disengage(&self) -> bool {
        let Some(engaged_at) = self.engaged_at else {
            return false;
        };
        if engaged_at.elapsed().as_secs() < self.disengage_threshold as u64 {
            return false;
        }
        !self
            .last_flap_at
            .is_some_and(|t| t.elapsed().as_secs() < self.disengage_threshold as u64)
    }

    /// Reset flap counter
    pub fn reset(&mut self) {
        self.flap_count = 0;
        self.engaged_at = None;
        self.disengage_timer = None;
        self.last_flap_at = None;
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
        // Announce RTBH for the actual peer IP (/32), not a synthesized /8.
        let announce = match config.peer_ip {
            std::net::IpAddr::V4(v4) => Some(announce_rtbh(
                &v4.to_string(),
                config.local_asn,
                config.peer_asn,
            )),
            std::net::IpAddr::V6(_) => None,
        };
        // Latch only a successfully encoded announce: an Err or an IPv6
        // peer (no RTBH message) must leave the trigger unlatched.
        if let Some(Ok(_)) = &announce {
            let now = std::time::Instant::now();
            trigger.engaged_at = Some(now);
            trigger.disengage_timer = Some(now);
        }
        announce
    } else if trigger.should_disengage() {
        trigger.reset();
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BgpConfig;

    #[test]
    fn test_schmitt_disengage_blocked_by_recent_flap() {
        let mut t = SchmittTrigger::new(1, 3);
        t.record_flap();
        t.engaged_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        assert!(!t.should_disengage(), "recent flap must suppress disengage");
        t.last_flap_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        assert!(t.should_disengage(), "quiet window must allow disengage");
    }

    #[test]
    fn test_schmitt_hysteresis() {
        // engage after 2 flaps, disengage after 3s of engagement
        let mut t = SchmittTrigger::new(2, 3);
        t.record_flap();
        assert!(!t.should_engage(), "one flap must not engage");
        t.record_flap();
        assert!(t.should_engage(), "two flaps must engage");

        // not engaged yet → cannot disengage
        assert!(!t.should_disengage(), "cannot disengage before engaging");

        // simulate successful engage
        t.engaged_at = Some(std::time::Instant::now());
        assert!(!t.should_disengage(), "fresh engagement must not disengage");
        t.engaged_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        t.last_flap_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        assert!(t.should_disengage(), "past disengage window must disengage");

        // saturating flap count never panics in debug builds
        let mut u = SchmittTrigger::new(1, 1);
        u.flap_count = u16::MAX;
        u.record_flap();
        assert_eq!(u.flap_count, u16::MAX);
    }

    #[test]
    fn apply_trigger_does_not_latch_ipv6_peer() {
        let mut cfg = BgpConfig {
            peer_ip: "2001:db8::1".parse().unwrap(),
            ..BgpConfig::default()
        };
        let mut t = SchmittTrigger::new(1, 3);
        t.record_flap();
        assert!(apply_trigger(&mut cfg, &mut t).is_none());
        assert!(t.engaged_at.is_none());
        assert!(t.disengage_timer.is_none());
    }

    #[test]
    fn apply_trigger_latches_successful_announce() {
        let mut cfg = BgpConfig::default();
        let mut t = SchmittTrigger::new(1, 3);
        t.record_flap();
        assert!(matches!(apply_trigger(&mut cfg, &mut t), Some(Ok(_))));
        assert!(t.engaged_at.is_some());
        assert!(t.disengage_timer.is_some());
    }
}
