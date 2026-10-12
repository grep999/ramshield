//! Condition-driven anti-entropy scheduling prototype.
//!
//! This module implements the per-peer scheduling policy used by the mesh
//! transport. The transport owns one planner per configured peer and invokes
//! `condition`, `take_due`, and `complete` around bounded snapshot/send work.
//! The scheduler is integrated, but its convergence guarantees remain a
//! prototype until network-fault and concurrent-pagination acceptance tests pass.
//!
//! The policy reduces needless full-state syncs while retaining a periodic
//! repair ceiling. Delta delivery is still separate from convergence: a socket
//! write is not an application acknowledgement.

use std::time::Duration;

/// Conditions that make a peer's view likely to need repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncCondition {
    /// Local state changed and should be coalesced with nearby changes.
    LocalMutation,
    /// A peer became reachable again; repair immediately.
    PeerReconnected,
    /// A delta or sync send failed; retry with bounded backoff.
    DeliveryFailed,
    /// Authenticated inbound work was dropped due to local queue pressure.
    QueueOverflow,
    /// A received delta was applied and local state may now differ from peers.
    RemoteStateApplied,
}

/// Why the planner scheduled an anti-entropy pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncReason {
    StateChanged,
    PeerReconnected,
    DeliveryRecovery,
    QueueOverflowRecovery,
    PeriodicRepair,
}

/// Policy bounds for one peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncPolicy {
    /// Batch nearby mutations rather than starting one full sync per mutation.
    pub coalesce_window: Duration,
    /// Maximum time between repair passes, even if no condition was observed.
    pub repair_interval: Duration,
    /// Initial delay after a failed sync/send.
    pub retry_initial: Duration,
    /// Upper bound on exponential retry delay.
    pub retry_max: Duration,
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self {
            coalesce_window: Duration::from_millis(50),
            repair_interval: Duration::from_secs(30),
            retry_initial: Duration::from_millis(250),
            retry_max: Duration::from_secs(10),
        }
    }
}

impl SyncPolicy {
    /// Reject configurations that can create a busy loop or disable repair.
    pub fn validate(self) -> Result<Self, &'static str> {
        if self.repair_interval.is_zero() {
            return Err("repair_interval must be non-zero");
        }
        if self.retry_initial.is_zero() {
            return Err("retry_initial must be non-zero");
        }
        if self.retry_max < self.retry_initial {
            return Err("retry_max must be >= retry_initial");
        }
        Ok(self)
    }
}

/// One planner instance per configured peer. Times are monotonic elapsed times
/// supplied by the caller (normally Tokio's `Instant` converted to elapsed
/// durations); wall-clock time must not be used for scheduling.
#[derive(Debug)]
pub struct ConditionSyncPlanner {
    policy: SyncPolicy,
    dirty_since: Option<Duration>,
    forced_reason: Option<SyncReason>,
    last_success: Option<Duration>,
    retry_at: Option<Duration>,
    consecutive_failures: u32,
    in_flight: bool,
}

impl ConditionSyncPlanner {
    pub fn new(policy: SyncPolicy) -> Result<Self, &'static str> {
        Ok(Self {
            policy: policy.validate()?,
            dirty_since: None,
            forced_reason: None,
            last_success: None,
            retry_at: None,
            consecutive_failures: 0,
            in_flight: false,
        })
    }

    /// Record a condition. Conditions arriving during an in-flight sync remain
    /// dirty and cause another pass after completion; they are never cleared by
    /// completion of an older snapshot.
    pub fn condition(&mut self, condition: SyncCondition, now: Duration) {
        self.dirty_since.get_or_insert(now);
        let reason = match condition {
            SyncCondition::LocalMutation | SyncCondition::RemoteStateApplied => {
                SyncReason::StateChanged
            }
            SyncCondition::PeerReconnected => SyncReason::PeerReconnected,
            SyncCondition::DeliveryFailed => SyncReason::DeliveryRecovery,
            SyncCondition::QueueOverflow => SyncReason::QueueOverflowRecovery,
        };

        // Preserve the most urgent trigger until a pass begins. Recovery and
        // reconnect conditions bypass mutation coalescing, but still respect
        // a failure backoff so an unreachable peer cannot cause a hot loop.
        if matches!(
            reason,
            SyncReason::PeerReconnected
                | SyncReason::DeliveryRecovery
                | SyncReason::QueueOverflowRecovery
        ) {
            self.forced_reason = Some(reason);
        } else if self.forced_reason.is_none() {
            self.forced_reason = Some(SyncReason::StateChanged);
        }
    }

    /// Claim a due pass. Returns None while another pass is in flight or a
    /// failure backoff has not elapsed. Caller must call `complete` exactly
    /// once for every returned reason, including cancellation and errors.
    pub fn take_due(&mut self, now: Duration) -> Option<SyncReason> {
        if self.in_flight || self.retry_at.is_some_and(|deadline| now < deadline) {
            return None;
        }

        let periodic_due = self
            .last_success
            .map(|last| now.saturating_sub(last) >= self.policy.repair_interval)
            .unwrap_or(true);
        let coalesced_due = self
            .dirty_since
            .is_some_and(|since| now.saturating_sub(since) >= self.policy.coalesce_window);

        let reason = if let Some(reason) = self.forced_reason {
            if reason != SyncReason::StateChanged || coalesced_due {
                Some(reason)
            } else {
                None
            }
        } else if periodic_due {
            Some(SyncReason::PeriodicRepair)
        } else {
            None
        }?;

        self.in_flight = true;
        self.dirty_since = None;
        self.forced_reason = None;
        Some(reason)
    }

    /// Complete a claimed pass. Failure keeps the peer dirty and applies capped
    /// exponential backoff. Success resets backoff; conditions recorded while
    /// the pass was in flight are intentionally retained.
    pub fn complete(&mut self, now: Duration, success: bool) {
        if !self.in_flight {
            return;
        }
        self.in_flight = false;
        if success {
            self.last_success = Some(now);
            self.retry_at = None;
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            let shift = self.consecutive_failures.saturating_sub(1).min(31);
            let multiplier = 1u32 << shift;
            let delay = self
                .policy
                .retry_initial
                .checked_mul(multiplier)
                .unwrap_or(self.policy.retry_max)
                .min(self.policy.retry_max);
            self.retry_at = Some(now.saturating_add(delay));
            self.dirty_since.get_or_insert(now);
            // A failed pass is more urgent than a coalesced state-change trigger.
            self.forced_reason = Some(SyncReason::DeliveryRecovery);
        }
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn planner() -> ConditionSyncPlanner {
        ConditionSyncPlanner::new(SyncPolicy {
            coalesce_window: ms(50),
            repair_interval: ms(1_000),
            retry_initial: ms(100),
            retry_max: ms(400),
        })
        .unwrap()
    }

    #[test]
    fn batches_mutations_until_coalesce_window() {
        let mut p = planner();
        p.condition(SyncCondition::LocalMutation, ms(0));
        p.condition(SyncCondition::LocalMutation, ms(10));
        assert_eq!(p.take_due(ms(49)), None);
        assert_eq!(p.take_due(ms(50)), Some(SyncReason::StateChanged));
        p.complete(ms(55), true);
        assert_eq!(p.take_due(ms(999)), None);
    }

    #[test]
    fn reconnect_bypasses_coalescing() {
        let mut p = planner();
        p.condition(SyncCondition::PeerReconnected, ms(10));
        assert_eq!(p.take_due(ms(10)), Some(SyncReason::PeerReconnected));
        p.complete(ms(20), true);
    }

    #[test]
    fn periodic_repair_covers_silent_missed_conditions() {
        let mut p = planner();
        assert_eq!(p.take_due(ms(0)), Some(SyncReason::PeriodicRepair));
        p.complete(ms(1), true);
        assert_eq!(p.take_due(ms(1_001)), Some(SyncReason::PeriodicRepair));
        p.complete(ms(1_002), true);
    }

    #[test]
    fn failed_send_retries_with_bounded_backoff() {
        let mut p = planner();
        p.condition(SyncCondition::DeliveryFailed, ms(0));
        assert_eq!(p.take_due(ms(0)), Some(SyncReason::DeliveryRecovery));
        p.complete(ms(0), false);
        assert_eq!(p.take_due(ms(99)), None);
        assert_eq!(p.take_due(ms(100)), Some(SyncReason::DeliveryRecovery));
        p.complete(ms(100), false);
        assert_eq!(p.take_due(ms(299)), None);
        assert_eq!(p.take_due(ms(300)), Some(SyncReason::DeliveryRecovery));
        p.complete(ms(300), false);
        assert_eq!(p.take_due(ms(699)), None);
        assert_eq!(p.take_due(ms(700)), Some(SyncReason::DeliveryRecovery));
        p.complete(ms(700), false);
        assert_eq!(p.consecutive_failures(), 4);
    }

    #[test]
    fn condition_during_inflight_pass_is_not_lost() {
        let mut p = planner();
        p.condition(SyncCondition::PeerReconnected, ms(0));
        assert_eq!(p.take_due(ms(0)), Some(SyncReason::PeerReconnected));
        p.condition(SyncCondition::LocalMutation, ms(1));
        p.complete(ms(10), true);
        assert_eq!(p.take_due(ms(51)), Some(SyncReason::StateChanged));
    }

    #[test]
    fn failed_pass_promotes_pending_work_to_recovery_reason() {
        let mut p = planner();
        p.condition(SyncCondition::PeerReconnected, ms(0));
        assert_eq!(p.take_due(ms(0)), Some(SyncReason::PeerReconnected));
        p.condition(SyncCondition::LocalMutation, ms(1));
        p.complete(ms(10), false);
        assert_eq!(p.take_due(ms(109)), None);
        assert_eq!(p.take_due(ms(110)), Some(SyncReason::DeliveryRecovery));
    }

    #[test]
    fn queue_overflow_requests_repair() {
        let mut p = planner();
        p.condition(SyncCondition::QueueOverflow, ms(0));
        assert_eq!(p.take_due(ms(0)), Some(SyncReason::QueueOverflowRecovery));
    }

    #[test]
    fn does_not_start_overlapping_syncs() {
        let mut p = planner();
        p.condition(SyncCondition::PeerReconnected, ms(0));
        assert!(p.take_due(ms(0)).is_some());
        assert_eq!(p.take_due(ms(1)), None);
        p.complete(ms(2), true);
    }

    #[test]
    fn rejects_zero_repair_and_invalid_retry_policy() {
        assert!(
            ConditionSyncPlanner::new(SyncPolicy {
                repair_interval: Duration::ZERO,
                ..SyncPolicy::default()
            })
            .is_err()
        );
        assert!(
            ConditionSyncPlanner::new(SyncPolicy {
                retry_initial: ms(200),
                retry_max: ms(100),
                ..SyncPolicy::default()
            })
            .is_err()
        );
    }
}
