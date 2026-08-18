//! Typed newtypes for time values to prevent mixing wall-clock and monotonic time.
//!
//! - [`EpochMillis`] / [`EpochNanos`]: UTC wall-clock timestamps for persisted/audit records.
//! - [`MonotonicNanos`]: Monotonic clock for local rate/TTL logic (immune to wall-clock jumps).

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum allowed clock skew: 5 minutes into the future.
const MAX_FUTURE_SKEW_NS: u64 = 5 * 60 * 1_000_000_000;

/// Maximum allowed age for incoming events: 24 hours.
const MAX_EVENT_AGE_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

/// UTC wall-clock timestamp in milliseconds since UNIX epoch.
/// Use only for persisted/audit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EpochMillis(pub u64);

/// UTC wall-clock timestamp in nanoseconds since UNIX epoch.
/// Use only for persisted/audit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EpochNanos(pub u64);

/// Monotonic timestamp in nanoseconds (relative to an arbitrary epoch).
/// Use for local rate/TTL logic — immune to wall-clock jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicNanos(pub u64);

impl EpochMillis {
    /// Current UTC wall-clock time as milliseconds since UNIX epoch.
    pub fn now() -> Self {
        Self(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        )
    }

    /// Convert to nanoseconds (multiplies by 1_000_000).
    pub fn as_nanos(self) -> EpochNanos {
        EpochNanos(self.0.saturating_mul(1_000_000))
    }

    /// Get the inner u64 value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl EpochNanos {
    /// Current UTC wall-clock time as nanoseconds since UNIX epoch.
    pub fn now() -> Self {
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        // Prevent truncation: as_nanos() returns u128, cap at u64::MAX
        Self(d.as_nanos().min(u64::MAX as u128) as u64)
    }

    /// Convert to milliseconds (divides by 1_000_000, truncating sub-ms precision).
    pub fn as_millis(self) -> EpochMillis {
        EpochMillis(self.0 / 1_000_000)
    }

    /// Get the inner u64 value.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Validate that this timestamp is within acceptable bounds.
    /// Returns `Err` if the timestamp is too far in the future or too old.
    pub fn validate(self) -> Result<(), TimestampError> {
        let now = Self::now().0;
        if self.0 > now.saturating_add(MAX_FUTURE_SKEW_NS) {
            return Err(TimestampError::TooFarFuture);
        }
        if now.saturating_sub(self.0) > MAX_EVENT_AGE_NS {
            return Err(TimestampError::TooOld);
        }
        Ok(())
    }
}

impl MonotonicNanos {
    /// Current monotonic time from an `Instant`.
    pub fn from_instant(instant: Instant, epoch: Instant) -> Self {
        let d = instant.duration_since(epoch);
        Self(d.as_nanos().min(u64::MAX as u128) as u64)
    }

    /// Duration since another monotonic timestamp.
    pub fn duration_since(self, earlier: MonotonicNanos) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    /// Get the inner u64 value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Errors from timestamp validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampError {
    /// Timestamp is too far in the future (> 5 minutes skew).
    TooFarFuture,
    /// Timestamp is too old (> 24 hours).
    TooOld,
}

impl std::fmt::Display for TimestampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimestampError::TooFarFuture => write!(f, "timestamp too far in the future"),
            TimestampError::TooOld => write!(f, "timestamp too old"),
        }
    }
}

impl std::error::Error for TimestampError {}

/// Helper to get current monotonic nanoseconds from an `Instant` relative to a stored epoch.
#[inline]
pub fn monotonic_now(epoch: Instant) -> MonotonicNanos {
    MonotonicNanos::from_instant(Instant::now(), epoch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_millis_now_is_reasonable() {
        let ms = EpochMillis::now();
        // Should be after 2020-01-01
        assert!(ms.0 > 1_577_836_800_000);
    }

    #[test]
    fn epoch_nanos_now_is_reasonable() {
        let ns = EpochNanos::now();
        // Should be after 2020-01-01
        assert!(ns.0 > 1_577_836_800_000_000_000);
    }

    #[test]
    fn epoch_millis_to_nanos_roundtrip() {
        let ms = EpochMillis(1_700_000_000_000);
        let ns = ms.as_nanos();
        assert_eq!(ns.0, 1_700_000_000_000_000_000);
        let back = ns.as_millis();
        assert_eq!(back.0, ms.0);
    }

    #[test]
    fn validate_accepts_current_time() {
        let now = EpochNanos::now();
        assert!(now.validate().is_ok());
    }

    #[test]
    fn validate_rejects_far_future() {
        let now = EpochNanos::now();
        let future = EpochNanos(now.0 + MAX_FUTURE_SKEW_NS + 1);
        assert_eq!(future.validate(), Err(TimestampError::TooFarFuture));
    }

    #[test]
    fn validate_rejects_too_old() {
        let now = EpochNanos::now();
        let old = EpochNanos(now.0.saturating_sub(MAX_EVENT_AGE_NS + 1));
        assert_eq!(old.validate(), Err(TimestampError::TooOld));
    }

    #[test]
    fn monotonic_nanos_duration() {
        let a = MonotonicNanos(1000);
        let b = MonotonicNanos(2000);
        let d = b.duration_since(a);
        assert_eq!(d, Duration::from_nanos(1000));
    }

    #[test]
    fn monotonic_nanos_saturating_sub() {
        let a = MonotonicNanos(2000);
        let b = MonotonicNanos(1000);
        let d = b.duration_since(a);
        assert_eq!(d, Duration::ZERO);
    }
}
