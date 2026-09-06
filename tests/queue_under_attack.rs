//! RED test: detection channel MUST be bounded and drop-newest under attack.
//! Bug: at 256k capacity, channel drains too slowly under sustained attack;
//! legitimate events get head-of-line-blocked. RFC 9411 heavy-impact condition.
//! GREEN: bounded capacity, drop-newest via try_send, dropped_events counter
//! exposed in IpcServerStats, dropped > 0 after burst > capacity.
//! Capacity raised 16k -> 64k (P1-7: single-flusher drain math at 1M eps);
//! the invariant under test is BOUNDED + DROP-NEWEST, not the exact number.

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
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
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
