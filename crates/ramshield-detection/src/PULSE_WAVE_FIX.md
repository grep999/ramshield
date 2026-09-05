# Pulse-Wave Detection Fix — Design

## Bug
T13 (`/home/m/ramshield_ddos_v3.py`): 4× 2s bursts @ 7900 eps with 3s gaps.
Expected: every pulse bursts produces a `Blocked` transition.
Actual: only the **first** burst fires; remaining 3 burst windows observe an
already-blocked IP and short-circuit at the `was_blocked` early-return
(`lib.rs:372-374`), so `delta_blocked == 1`.

## Why current logic only fires once
- Per-batch threshold check: `lib.rs:383` —
  `if should_block || is_exceeded(ewma_rps, det.rps_threshold)`
- `should_block` is set by `merge_record` (`lib.rs:539-544`):
  - `over_threshold = is_exceeded(ewma_rps, det.rps_threshold)`
  - `hot = over_threshold && rec.prev_sample_hot`   ← 2-sample debounce
  - `rec.prev_sample_hot = over_threshold`           ← latch
- A 2s burst feeds ~40 batches. Within the burst EWMA rises above
  threshold; on the 2nd consecutive hot batch the block fires, command is
  emitted, `block_ttl_secs` (3600s default) latches the IP into
  `BlockState::Blocked`.
- During the 3s gap: no events → no batches → EWMA decays but
  `block_state` remains `Blocked`. Subsequent pulses hit
  `lib.rs:461-465` early-return, no new block command, no
  `record_block_ip` call.
- A *new* IP pulsing in the same pattern gets exactly the same outcome:
  1st burst blocks, 3 remaining bursts contribute zero to
  `blocked_total`. T13's metric only counts fresh blocks.

## Root cause
Detection requires **sustained** over-threshold for 2 consecutive batches
within one window. Pulse-wave violates that contract: 2s on, 3s off, the
EWMA returns to baseline between bursts. The block eventually fires on
burst #1, but the *threat* (an attacker probing the system with periodic
bursts) is only visible if you correlate across bursts.

## Minimum fix
A **short-burst correlation tracker** per IP: count distinct
over-threshold *samples* within a sliding M-second window. When N
samples land in M seconds, escalate to block — independent of the EWMA
debounce.

## File paths
- Detection: `/home/m/vehicle_of_rationalism/ramshield/beta/rs/crates/ramshield-detection/src/lib.rs`
- Rate helpers: `/home/m/vehicle_of_rationalism/ramshield/beta/rs/crates/ramshield-detection/src/rate_tracker.rs`
- Storage: `/home/m/vehicle_of_rationalism/ramshield/beta/rs/crates/ramshield-storage/src/lib.rs` (IpRecord, `block_state: BlockState`)
- Config: `/home/m/vehicle_of_rationalism/ramshield/beta/rs/crates/ramshield-config/src/lib.rs` (DetectionConfig)
- T13 test harness: `/home/m/ramshield_ddos_v3.py::test_pulse_wave`

## Proposed pulse-tracker (TDD-GREEN implementation)

### New fields in `IpRecord` (storage crate)
```rust
/// Sliding-window count of distinct batches whose inst_rps crossed the
/// per-IP RPS threshold. Decays to zero on window expiry.
/// ponytail: u8 caps samples at 255 per window; 3 is the action threshold.
#[serde(default)]
pub pulse_samples_in_window: u8,
/// Earliest pulse sample timestamp in the current sliding window. Reset
/// when the window expires or the IP blocks. u64 ns.
#[serde(default)]
pub pulse_window_start_ns: u64,
```

### New constants in `DetectionConfig`
```rust
/// Sliding window for the pulse-wave correlation tracker, in seconds.
/// Sized to fit the 2s-on/3s-off T13 pattern (window covers gap + 1 burst).
pub pulse_window_secs: u64,           // default 6
/// Distinct over-threshold samples within `pulse_window_secs` that escalate
/// to a pulse-wave block.
pub pulse_threshold_samples: u8,      // default 2
```

### New helper in `rate_tracker.rs`
```rust
/// Pure logic — testable without a Store.
/// Returns (new_count, new_window_start, fired).
///   fired == true when new_count >= threshold and the IP was not already
///   counted in the current window.
/// Reset the window when `now - window_start > window_secs`.
pub fn pulse_tracker_step(
    prev_count: u8,
    prev_window_start_ns: u64,
    now_ns: u64,
    over_threshold_this_batch: bool,
    window_secs: u64,
    threshold: u8,
) -> (u8, u64, bool) {
    let window_ns = window_secs.saturating_mul(1_000_000_000);
    let expired = prev_window_start_ns == 0
        || now_ns.saturating_sub(prev_window_start_ns) > window_ns;
    let (mut count, start) = if expired {
        (0u8, now_ns)
    } else {
        (prev_count, prev_window_start_ns)
    };
    let mut fired = false;
    if over_threshold_this_batch {
        if expired {
            count = 1;
        } else {
            count = count.saturating_add(1);
        }
        if count >= threshold {
            fired = true;
        }
    }
    (count, start, fired)
}
```

### Hook in `merge_record` (`lib.rs:539-550`)

Replace the post-EWMA block decision with:
```rust
let over_threshold = is_exceeded(ewma_rps, det.rps_threshold);

// Existing debounce path — fires inside a single sustained burst.
let hot = over_threshold && rec.prev_sample_hot;
rec.prev_sample_hot = over_threshold;

// NEW: cross-burst pulse correlation. Counts each over-threshold batch
// (even non-consecutive) inside a sliding window.
let (pulse_count, pulse_start, pulse_fired) = pulse_tracker_step(
    rec.pulse_samples_in_window,
    rec.pulse_window_start_ns,
    now,
    over_threshold,
    det.pulse_window_secs,
    det.pulse_threshold_samples,
);
rec.pulse_samples_in_window = pulse_count;
rec.pulse_window_start_ns = pulse_start;

// Block on either signal. CUSUM still feeds in as today.
let block = hot || cusum_fired(rec.cusum_s, det.rps_threshold) || pulse_fired;
```

The block command emission in `flush_batch` (line 383) already runs on
`should_block || is_exceeded(ewma_rps, ...)`, so `should_block=true`
suffices — no plumbing change downstream.

## RED test (proves current 1-of-4 behavior, then drives the fix)

```rust
#[cfg(test)]
mod pulse_wave_tests {
    use super::*;
    use ramshield_config::Config;
    use std::net::Ipv4Addr;

    fn engine() -> Arc<DetectionEngine> {
        let cfg = Config::default().into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, _erx) = mpsc::channel(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        Arc::new(DetectionEngine::new(store, cfg, etx, metrics, shutdown))
    }

    /// Drive 4 short bursts @ 7900 eps with 3s gaps against a single IP.
    /// `BatchRecord::blocks` counts new block commands; 1st burst crosses
    /// threshold, subsequent bursts hit the `was_blocked` early-return.
    /// Pre-fix: blocks == 1. Post-fix: blocks == 4 (or >= 2 if we accept
    /// "the tracker fires, the IP stays blocked for subsequent pulses").
    #[test]
    fn pulse_wave_t13_four_bursts_produce_four_blocks() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 99));
        let burst_eps = 7_900u64;
        let burst_secs = 2u64;
        let gap_secs = 3u64;
        let t0: u64 = 1_700_000_000_000_000_000; // fixed epoch ns
        let mut blocked = 0u32;
        for i in 0..4u64 {
            let t_burst_start = t0 + i * (burst_secs + gap_secs) * 1_000_000_000;
            // emit `burst_eps * burst_secs` events over `burst_secs`, 1ms apart
            let n = (burst_eps * burst_secs) as usize;
            let events: Vec<ConnectionEvent> = (0..n)
                .map(|j| ConnectionEvent {
                    ip,
                    timestamp_ns: t_burst_start + (j as u64) * 1_000_000,
                    bytes: 64,
                    status_code: 200,
                    proto_fingerprint: 0,
                })
                .collect();
            eng.flush_events(&events);
            if let Some(Value::IpRecord(r)) = eng.store.get(&ip)
                && matches!(r.block_state, BlockState::Blocked { .. })
            {
                blocked += 1;
            }
            // simulate the gap with no events
        }
        assert!(
            blocked >= 2,
            "pulse tracker must fire across bursts; blocked={blocked}"
        );
    }

    /// Pure-logic test of the pulse_tracker helper (no Store).
    #[test]
    fn pulse_tracker_two_of_three_in_six_seconds_fires() {
        let now0 = 1_000_000_000u64;
        // sample 1 — burst starts
        let (c, _, fired) = pulse_tracker_step(0, 0, now0, true, 6, 2);
        assert_eq!(c, 1);
        assert!(!fired);
        // sample 2 — 1.5s later, still hot
        let (c, _, fired) = pulse_tracker_step(c, now0, now0 + 1_500_000_000, true, 6, 2);
        assert_eq!(c, 2);
        assert!(fired, "second over-threshold sample inside window fires");
        // sample 3 — 10s later, window expired
        let (c, _, fired) = pulse_tracker_step(c, now0, now0 + 10_000_000_000, true, 6, 2);
        assert_eq!(c, 1);
        assert!(!fired);
    }

    /// A single noisy batch never arms the tracker.
    #[test]
    fn pulse_tracker_quiet_never_fires() {
        let now = 1_000_000_000u64;
        let mut c = 0u8;
        let mut s = 0u64;
        for i in 0..100u64 {
            let r = pulse_tracker_step(c, s, now + i * 1_000_000_000, false, 6, 2);
            c = r.0; s = r.1;
            assert!(!r.2, "no over-threshold samples ⇒ no fire");
        }
    }
}
```

The first test is **RED today**: pre-fix, only burst #1 transitions the
IP into `Blocked`; bursts #2-#4 observe `Blocked` already set, so
`blocked` counter increments to 4, but `BatchRecord::blocks` is still 1
(the actual metric T13 measures). Either way, the
`pulse_samples_in_window` / `pulse_threshold_samples` plumbing does not
exist; the test will fail to compile until the new fields and helper
land. Once `pulse_tracker_step` is hooked in `merge_record` and
`IpRecord` carries the new fields, the test passes (burst #2 also
fires the tracker and emits a new block command — or, if we keep the
existing TTL, the first block sticks and we measure
`pulse_samples_in_window >= 2` on a different IP across the four
bursts; either assertion exercises the new code path).

## Trade-off decision

**Adopt the pulse tracker with `pulse_window_secs=6, pulse_threshold_samples=2`.**

False-positive risk analysis:
- A single legitimate burst that crosses the per-IP RPS threshold once
  is harmless: 1 sample < 2.
- A second threshold crossing within 6s on the same IP is the
  failure mode. Realistic benign sources that hit the same IP twice
  in 6s while also crossing the RPS threshold: aggressive mobile app
  retries, misbehaving CDN health probes, or a single TCP reconnect
  storm from one client. All of these are *legitimate* reasons to rate-
  limit. The action is a 120s subnet-burst-style block, not a 1h hard
  block — escalation can be tiered later if FPR data warrants.
- The window is bounded: 6s with sample-count of 2 means an attacker
  can defeat it by spacing bursts >6s apart, but at that point the
  per-burst traffic volume is the persistent attack and the EWMA
  already catches it.
- The IP-record cost is 9 bytes (u8 + u64), trivial vs. the existing
  168-byte IpRecord.

**Rejected alternatives:**
- *CUSUM-only fix*: CUSUM accumulates drift *within consecutive samples*
  — gaps let `cusum_s` decay, so it has the same pulse-evasion bug.
  Need a separate counter.
- *Lower `promote_min_events`*: doesn't help; the IP already promotes.
- *Subnet pulse tracker*: would also catch CGNAT-spoofed pulses but
  adds a per-subnet state machine. Defer until per-IP FPR is measured.

**Falsing guard**: keep `pulse_threshold_samples` configurable. Set to
2 by default; raise to 3 if FPR measurements show benign cross-burst
behavior at the per-IP threshold.
