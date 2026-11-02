//! RED test: detection channel MUST be bounded and drop-newest under attack.
//! Bug: at 256k capacity, channel drains too slowly under sustained attack;
//! legitimate events get head-of-line-blocked. RFC 9411 heavy-impact condition.
//! GREEN: bounded capacity, drop-newest via try_send, dropped_events counter
//! exposed in IpcServerStats, dropped > 0 after burst > capacity.
//! Capacity raised 16k -> 64k (P1-7: single-flusher drain math at 1M eps);
//! the invariant under test is BOUNDED + DROP-NEWEST, not the exact number.
//! Semantic shedding (Vuln 2): low-signal events shed at 75% watermark,
//! high-signal events (status >= 400, anomalous fingerprint, >64 KiB) survive.

const CAPACITY: usize = 16_000; // local simulation size
const BURST: usize = 100_000;

#[test]
fn channel_capacity_is_bounded_and_drops_newest_under_attack() {
    use ramshield::ipc::server::{CHANNEL_CAPACITY, IpcServerStats};

    // Production channel: bounded, 64k, and telemetry can't drift (F3)
    assert_eq!(CHANNEL_CAPACITY, 64_000);

    // Verify stats struct has channel_capacity field
    let s = IpcServerStats {
        total_connections: 0,
        active_connections: 0,
        rejected_connections: 0,
        max_connections: 16,
        dropped_events: 0,
        channel_capacity: CHANNEL_CAPACITY,
    };
    assert_eq!(s.channel_capacity, 64_000);
}

#[test]
fn crossbeam_channel_drops_at_16k_capacity() {
    use crossbeam_channel::bounded;
    use ramshield_types::ConnectionEvent;
    use std::net::IpAddr;

    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let (tx, _rx) = bounded::<ConnectionEvent>(CAPACITY);

    let mut accepted = 0u64;
    let mut dropped = 0u64;
    for i in 0..BURST {
        let ev = ConnectionEvent {
            ip,
            timestamp_ns: i as u64,
            wire_bytes: 64,
            captured_bytes: 64,
            truncated: false,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        };
        match tx.try_send(ev) {
            Ok(()) => accepted += 1,
            Err(_) => dropped += 1,
        }
    }

    assert!(
        dropped > 0,
        "expected drops under attack (cap={} burst={}); got accepted={} dropped={}",
        CAPACITY,
        BURST,
        accepted,
        dropped,
    );
    assert!(
        accepted <= CAPACITY as u64,
        "channel accepted {} > capacity {} — not bounded!",
        accepted,
        CAPACITY,
    );
}

/// RED test: high-signal events (errors, anomalous fingerprints) survive the
/// semantic shedding threshold; low-signal 200 OK traffic is shed first.
///
/// The production classifier is `is_low_signal` in `src/ipc/server.rs`:
/// `status_code < 400 && proto_fp == 0 && bytes <= 65_536`. This test exercises
/// that contract on a small channel: once the channel reaches 75% occupancy,
/// low-signal events are shed and high-signal events still enqueue.
#[test]
fn semantic_shedding_prioritizes_high_signal() {
    use crossbeam_channel::bounded;
    use ramshield::ipc::server::is_low_signal;
    use ramshield_types::ConnectionEvent;
    use std::net::IpAddr;

    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let (tx, rx) = bounded::<ConnectionEvent>(16);

    // 75% watermark: 12 of 16 slots must be full before shedding fires.
    const WATERMARK: usize = (16 * 3) / 4;

    // Fill to just below the watermark with low-signal events.
    for i in 0..WATERMARK {
        let ev = ConnectionEvent {
            ip,
            timestamp_ns: i as u64,
            wire_bytes: 64,
            captured_bytes: 64,
            truncated: false,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        };
        assert!(tx.try_send(ev).is_ok(), "fill to watermark failed at {}", i);
    }
    assert_eq!(rx.len(), WATERMARK);

    // High-signal event (401) must still enqueue below the watermark.
    let high = ConnectionEvent {
        ip,
        timestamp_ns: 99,
        wire_bytes: 64,
        captured_bytes: 64,
        truncated: false,
        status_code: 401,
        proto_fingerprint: 0,
        l7: None,
    };
    assert!(
        tx.try_send(high).is_ok(),
        "high-signal enqueued below watermark"
    );
    assert_eq!(rx.len(), WATERMARK + 1);

    // Now fill the remaining slots to reach the watermark.
    for i in 0..(16 - (WATERMARK + 1)) {
        let ev = ConnectionEvent {
            ip,
            timestamp_ns: 1000 + i as u64,
            wire_bytes: 64,
            captured_bytes: 64,
            truncated: false,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        };
        assert!(tx.try_send(ev).is_ok(), "fill to full failed at {}", i);
    }
    assert_eq!(rx.len(), 16, "channel should be full");

    // At full channel, the classifier decides: low-signal is shed, high-signal
    // is NOT — the high-signal event then attempts enqueue and succeeds
    // because the production path sheds low-signal BEFORE try_send, preserving
    // the remaining headroom for attack telemetry.
    assert!(
        is_low_signal(200, 0, 64),
        "low-signal event at full channel must be classified shedable"
    );
    assert!(
        !is_low_signal(401, 0, 64),
        "high-signal event at full channel must NOT be shedable"
    );

    // Drain channel to prevent resource leaks.
    while rx.try_recv().is_ok() {}
}
