use super::*;

/// Patch B: a pulsed swarm must not reset mid-attack. Two merges 3s
/// apart (inside the widened window) must accumulate, not zero out.
#[test]
fn subnet_window_survives_pulsed_swarm_within_window() {
    let store = Store::new(16);
    let t0 = 1_000_000_000u64;
    let mk = |o: u8| IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, o));
    let any = mk(1);
    let net = IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();

    let first: Vec<IpAddr> = (1..=30u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 60, Some(&first), t0);
    // Second pulse 3s later: 30 new hosts, 60 more events.
    let second: Vec<IpAddr> = (31..=60u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 60, Some(&second), t0 + 3_000_000_000);

    let rec = store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.unique_ips(),
        60,
        "host bitmap must accumulate across a 3s pulse gap"
    );
    assert_eq!(
        rec.total_rps, 120,
        "event volume must accumulate across a 3s pulse gap"
    );
}

/// Patch B boundary: accumulation must still end past the widened window.
/// A never-resetting window would let a once-hot /24 block forever.
#[test]
fn subnet_window_resets_beyond_widened_boundary() {
    let store = Store::new(16);
    let t0 = 1_000_000_000u64;
    let mk = |o: u8| IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, o));
    let any = mk(1);
    let net = IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();

    let hosts: Vec<IpAddr> = (1..=60u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 480, Some(&hosts), t0);
    // Well past the widened boundary: signal must be gone.
    store.merge_subnet_window(
        sk,
        net,
        3,
        Some(&[mk(200)]),
        t0 + SUBNET_WINDOW_NS + 1_000_000_000,
    );
    let rec = store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.unique_ips(),
        1,
        "stale swarm signal must not survive past the window"
    );
    assert_eq!(rec.total_rps, 3);
}

/// NTP backward step: a merge stamped BEFORE the previous one must not
/// regress the window baseline. The baseline stays at the high-water
/// mark (events still count — no reset, no loss), and the window still
/// expires on a later forward timestamp.
#[test]
fn subnet_window_clock_step_backwards_keeps_baseline() {
    let store = Store::new(16);
    let t0 = 10_000_000_000u64;
    let mk = |o: u8| IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, o));
    let any = mk(1);
    let net = IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();

    let hosts: Vec<IpAddr> = (1..=30u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 60, Some(&hosts), t0);
    // Clock steps back 4s (≈ one window): the window must NOT reset on
    // this merge (decay = 0, not "window just started"), and the
    // baseline must NOT regress to the stepped timestamp.
    let stepped = t0 - 4_000_000_000;
    store.merge_subnet_window(sk, net, 60, Some(&[mk(200)]), stepped);
    let rec = store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.last_updated_ns, t0,
        "clock step must not regress the baseline"
    );
    assert_eq!(
        rec.total_rps, 120,
        "stepped merge must accumulate, not reset or drop events"
    );
    drop(rec);
    // Forward time must still expire the window.
    store.merge_subnet_window(
        sk,
        net,
        3,
        Some(&[mk(201)]),
        t0 + SUBNET_WINDOW_NS + 1_000_000_000,
    );
    let rec = store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.total_rps, 3,
        "window must expire normally after the step"
    );
}

/// Test helper: create an IpRecord with `block_state = Blocked`.
fn blocked_record(ip: IpAddr) -> IpRecord {
    IpRecord {
        ip,
        request_count: 1,
        ewma_rps: 0.0,
        cusum_s: 0.0,
        baseline_rps: 0.0,
        prev_sample_hot: false,
        sample_count: 0,
        relative_breach_streak: 0,
        pulse_samples_in_window: 0,
        pulse_window_start_ns: 0,
        first_seen_ns: 0,
        last_seen_ns: 0,
        bytes_in: 0,
        status_dist: [0; 5],
        proto_fingerprint: 0,
        threat_score: 0.0,
        block_state: BlockState::Blocked {
            reason: ramshield_types::BlockReason::HighRps,
            since_ns: 0,
        },
    }
}

/// P1 regression: a CapacityExceeded rollback removes the entry from
/// `inner` but must NOT leave the blocked indexes incremented. Before
/// the fix, blocked accounting ran before the capacity gate, so every
/// denied blocked-insert permanently inflated blocked_count and
/// blocked_set — phantom IPs that `get_all_blocked_ips` (and downstream
/// unblock-all) would act on.
/// The decision is the block; members are views of it. A member of an
/// active CIDR block has no IpRecord of its own (256 records for one
/// decision is the wrong shape), so a query that reads only per-IP state
/// lies. This asserts the shared clock: the same `active_cidrs` the
/// enforcement actor writes is what the query path must consult.
#[test]
fn cidr_block_visible_to_ip_query() {
    let store = Store::new(16);
    let net = IpNetwork::new("172.16.30.0".parse().unwrap(), 24).unwrap();
    store.active_cidrs.insert(net, ());

    let member: IpAddr = "172.16.30.44".parse().unwrap();
    // Precondition: no per-IP record exists for the member.
    assert!(store.get(&member).is_none(), "member has no IpRecord");
    // The CIDR clock must answer for it.
    assert_eq!(
        store.is_blocked_by_cidr(&member),
        Some(net),
        "member of active /24 must read as blocked"
    );
    // Outsiders stay clean.
    let outside: IpAddr = "172.16.31.1".parse().unwrap();
    assert!(store.is_blocked_by_cidr(&outside).is_none());
}

#[test]
fn capacity_denial_does_not_pollute_blocked_indexes() {
    let store = Store::new(16);
    let ip: IpAddr = "10.9.9.9".parse().unwrap();
    // ~1 byte budget: any blocked IpRecord insert must be denied.
    let err = store
        .insert(ip, Value::IpRecord(blocked_record(ip)), None, 1)
        .unwrap_err();
    assert!(matches!(err, RsError::CapacityExceeded { .. }));
    assert_eq!(
        store.get_stats().blocked,
        0,
        "blocked_count leaked on rollback"
    );
    assert!(
        store.get_all_blocked_ips().is_empty(),
        "blocked_set leaked on rollback"
    );
    assert!(
        !store.inner().contains_key(&ip),
        "entry itself must be rolled back"
    );

    // Control: a successful blocked insert DOES register in both indexes.
    store
        .insert(
            ip,
            Value::IpRecord(blocked_record(ip)),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
    assert_eq!(store.get_stats().blocked, 1);
    assert_eq!(store.get_all_blocked_ips(), vec![ip]);
}

/// Documented architecture contract, commit item: a fresh insert that
/// exceeds the budget is rejected BEFORE publication — no phantom entry,
/// no accounting, no index trace anywhere.
#[test]
fn rejected_insert_leaves_no_phantom() {
    let store = Store::new(16);
    let ip: IpAddr = "10.9.9.8".parse().unwrap();
    let denied = store
        .insert(ip, Value::IpRecord(blocked_record(ip)), None, 1)
        .unwrap_err();
    assert!(matches!(denied, RsError::CapacityExceeded { .. }));
    assert!(!store.inner().contains_key(&ip), "no phantom entry");
    assert_eq!(store.ram_bytes(), 0, "no accounting on a rejected insert");
    assert_eq!(
        store.get_stats().blocked,
        0,
        "no blocked index on a rejected insert"
    );
    assert!(store.get_all_blocked_ips().is_empty());
}

/// Documented architecture contract, commit item: the capacity gate runs
/// under the shard lock, so under concurrent fresh inserts the budget is
/// never silently exceeded and no rejected entry ever persists.
#[test]
fn capacity_gate_holds_under_concurrent_fresh_inserts() {
    let store = std::sync::Arc::new(Store::new(16));
    // Budget for ~50 records (blank record + key ~ 128 B): 16 KiB.
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let s = std::sync::Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            for i in 0..200u32 {
                let ip: IpAddr =
                    std::net::Ipv4Addr::new((t % 250) as u8, (i % 250) as u8, 0, 1).into();
                let _ = s.insert(ip, Value::IpRecord(blank_record(ip)), None, 16 * 1024);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert!(
        store.ram_bytes() <= 16 * 1024,
        "budget silently exceeded: {} bytes",
        store.ram_bytes()
    );
    assert!(store.ram_bytes() > 0, "some inserts must have committed");
    // Every surviving entry must account for real bytes: total equals the
    // sum of the live entries (no phantom residue from rollbacks).
    let sum: usize = store
        .inner()
        .iter()
        .map(|e| {
            std::mem::size_of::<Entry>() + e.value.heap_bytes() + std::mem::size_of::<IpAddr>()
        })
        .sum();
    assert_eq!(
        store.ram_bytes(),
        sum,
        "accounting diverged from live entries"
    );
}

#[test]
fn inline_for_small() {
    let v = Value::from_bytes(&[1u8; 10]);
    assert!(matches!(v, Value::Inline(_)));
}

#[test]
fn blob_for_large() {
    let v = Value::from_bytes(&[1u8; 100]);
    assert!(matches!(v, Value::Blob(_)));
}

#[test]
fn insert_get_remove() {
    let store = Store::new(16);
    store
        .insert(
            "127.0.0.1".parse().unwrap(),
            Value::Counter(1),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
    assert!(store.get(&"127.0.0.1".parse().unwrap()).is_some());
    store.remove(&"127.0.0.1".parse().unwrap());
    assert!(store.get(&"127.0.0.1".parse().unwrap()).is_none());
}

#[test]
fn ttl_lazy_expiry() {
    let store = Store::new(16);
    store
        .insert(
            "127.0.0.3".parse().unwrap(),
            Value::Counter(1),
            Some(0),
            64 * 1024 * 1024,
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(store.get(&"127.0.0.3".parse().unwrap()).is_none());
}

/// IPv6 plan Task 1: subnet records must carry family-complete CIDR
/// metadata. The old `prefix: [u8;3]` (v4-shaped) rendered v6 /64s as
/// garbage three-octet strings in the dashboard and batch-block logs.
#[test]
fn subnet_record_cidr_display_both_families() {
    let store = Store::new(8);
    let v4: IpAddr = "198.51.100.7".parse().unwrap();
    let v6: IpAddr = "2001:db8:abcd::5".parse().unwrap();
    let (k4, n4) = subnet::subnet_key(v4).unwrap();
    let (k6, n6) = subnet::subnet_key(v6).unwrap();
    store.merge_subnet_window(k4, n4, 5, Some(&[v4]), 0);
    store.merge_subnet_window(k6, n6, 5, Some(&[v6]), 0);
    assert_eq!(store.subnet_cidr(k4), "198.51.100.0/24");
    assert_eq!(store.subnet_cidr(k6), "2001:db8:abcd::/64");
}

#[test]
fn subnet_window_v6_key() {
    let store = Store::new(16);
    let v6: IpAddr = "2001:db8::1".parse().unwrap();
    let key = subnet::subnet_key_u128(v6).unwrap();
    let net = IpNetwork::of_ip(v6);
    store.merge_subnet_window(key, net, 5, Some(&[v6]), 1_000_000_000);
    assert_eq!(store.subnet_table().get(&key).unwrap().total_rps, 5);
    store.reset_subnet_window(key);
    assert_eq!(store.subnet_table().get(&key).unwrap().total_rps, 0);
}

/// P0 regression: capacity check used to be racy — two threads both
/// reading pre-insert `ram_bytes` could both decide "fits" and both
/// insert, silently exceeding the budget. The CAS-based reservation
/// ensures only one thread succeeds; the other rolls back.
#[test]
fn capacity_race_serializes_via_cas() {
    use std::sync::Arc;
    use std::thread;
    let store = Arc::new(Store::new(16));
    // Tight budget: 1 KiB net-new total.
    let limit = 1024;
    let mut handles = vec![];
    // 64 distinct IPs, each with a 64-byte payload (size_of::<Entry> +
    // 64 bytes value). 64 × ~200B = ~13 KiB > 1 KiB limit.
    // Some inserts must fail with CapacityExceeded; the survivors must
    // leave ram_bytes ≤ limit.
    for n in 0..64u8 {
        let s = store.clone();
        handles.push(thread::spawn(move || {
            let ip: IpAddr = format!("10.1.1.{}", n).parse().unwrap();
            s.insert(ip, Value::Inline(vec![0u8; 64]), None, limit)
        }));
    }
    let mut ok = 0;
    let mut denied = 0;
    for h in handles {
        match h.join().unwrap() {
            Ok(_) => ok += 1,
            Err(RsError::CapacityExceeded { .. }) => denied += 1,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
    assert!(denied > 0, "some inserts must be denied at 1KiB / 64 IPs");
    assert_eq!(ok + denied, 64);
    // Critical: ram_bytes must not exceed the limit. The old racy code
    // could let the budget be silently blown here.
    let used = store.ram_bytes();
    assert!(
        used <= limit,
        "ram_bytes {} must not exceed limit {} after race",
        used,
        limit
    );
}

/// P0 regression: subnet_index must be cleaned on eviction/removal.
/// Without this, every churned IP leaves a phantom in its subnet's
/// DashSet, growing the index unboundedly.
#[test]
fn subnet_index_cleaned_on_evict_and_remove() {
    let store = Store::new(16);
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let sk = subnet_key_u128(ip).unwrap();
    let net = IpNetwork::of_ip(ip);
    // Build a real record in `inner` AND populate subnet_index — this
    // is the steady-state shape detection creates.
    store
        .insert(
            ip,
            Value::IpRecord(crate::IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                cusum_s: 0.0,
                baseline_rps: 0.0,
                prev_sample_hot: false,
                sample_count: 0,
                relative_breach_streak: 0,
                pulse_samples_in_window: 0,
                pulse_window_start_ns: 0,
                first_seen_ns: 0,
                last_seen_ns: 0,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: 0,
                threat_score: 0.0,
                block_state: crate::BlockState::Clean,
            }),
            Some(0),
            1 << 20,
        )
        .unwrap();
    store.update_subnet_index(ip, Some(sk), false);
    // Also seed the subnet window so the same subnet is recognized.
    store.merge_subnet_window(sk, net, 1, Some(&[ip]), 1_000_000_000);
    assert_eq!(store.subnet_member_count(sk), 1);

    // evict_batch: TTL already expired by the time we call.
    std::thread::sleep(std::time::Duration::from_millis(2));
    store.evict_batch(&[ip]);
    assert!(
        store.subnet_member_count(sk) == 0,
        "subnet_index must be empty after evict_batch; stale entries leak memory"
    );
}

/// P0 regression: Store::remove must also clean subnet_index, or any
/// caller that removes an IP directly (e.g. dashboard unblock endpoint)
/// leaves a phantom in the reverse index.
#[test]
fn subnet_index_cleaned_on_remove() {
    let store = Store::new(16);
    let ip: IpAddr = "10.0.0.2".parse().unwrap();
    let sk = subnet_key_u128(ip).unwrap();
    store.insert(ip, Value::Counter(1), None, 1 << 20).unwrap();
    store.update_subnet_index(ip, Some(sk), false);
    assert_eq!(store.subnet_member_count(sk), 1);
    store.remove(&ip);
    assert!(
        store.subnet_member_count(sk) == 0,
        "subnet_index must be empty after remove; stale entries leak memory"
    );
}

/// P0 regression: get_all_blocked_ips must be O(blocked) not O(store).
/// Old code did `self.inner.iter().filter(|e| e.value().is_blocked())`
/// which walks every IP in the store. The fix maintains a `blocked_set`
/// DashSet updated on BlockState transitions, making the lookup O(B).
/// This test plants 1000 clean IPs and 5 blocked, asserts the function
/// returns exactly the 5 blocked IPs.
#[test]
fn get_all_blocked_ips_uses_index() {
    let store = Store::new(16);
    // 1000 clean IPs
    for n in 0..1000u16 {
        let ip: IpAddr = format!("10.5.{}.{}", n / 256, n % 256).parse().unwrap();
        store
            .insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
            .unwrap();
    }
    // 5 blocked IPs
    let blocked_ips: Vec<IpAddr> = (0..5u16)
        .map(|n| format!("10.99.0.{}", n).parse().unwrap())
        .collect();
    for ip in &blocked_ips {
        store
            .insert(
                *ip,
                Value::IpRecord(blocked_record(*ip)),
                None,
                64 * 1024 * 1024,
            )
            .unwrap();
    }
    let got: std::collections::HashSet<IpAddr> = store.get_all_blocked_ips().into_iter().collect();
    let want: std::collections::HashSet<IpAddr> = blocked_ips.into_iter().collect();
    assert_eq!(
        got, want,
        "get_all_blocked_ips must return exactly the blocked set"
    );
    assert_store_invariants(&store);
}

/// P0 regression: get_all_blocked_ips must reflect unblock transitions.
/// Insert blocked, then replace with clean — set should remove the IP.
#[test]
fn get_all_blocked_ips_tracks_unblock() {
    let store = Store::new(16);
    let ip: IpAddr = "10.6.6.6".parse().unwrap();
    store
        .insert(
            ip,
            Value::IpRecord(blocked_record(ip)),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
    assert_eq!(store.get_all_blocked_ips(), vec![ip]);
    // Replace with clean
    store
        .insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
        .unwrap();
    assert!(store.get_all_blocked_ips().is_empty());
    assert_store_invariants(&store);
}

/// P0 regression: blocked_set must stay in sync across all 4 mutation paths.
/// Insert, replace (clean→blocked), replace (blocked→clean), remove.
#[test]
fn blocked_set_consistent_across_mutations() {
    let store = Store::new(16);
    let ip: IpAddr = "10.7.7.7".parse().unwrap();
    // 1. Insert clean — set must NOT contain ip.
    store
        .insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
        .unwrap();
    assert!(store.get_all_blocked_ips().is_empty());
    // 2. Replace clean→blocked — set must contain ip.
    store
        .insert(
            ip,
            Value::IpRecord(blocked_record(ip)),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
    assert_eq!(store.get_all_blocked_ips(), vec![ip]);
    // 3. Replace blocked→clean — set must NOT contain ip.
    store
        .insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
        .unwrap();
    assert!(store.get_all_blocked_ips().is_empty());
    // 4. Re-block, then remove — set must NOT contain ip.
    store
        .insert(
            ip,
            Value::IpRecord(blocked_record(ip)),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
    assert_eq!(store.get_all_blocked_ips(), vec![ip]);
    store.remove(&ip);
    assert!(store.get_all_blocked_ips().is_empty());
    assert_store_invariants(&store);
}

/// P0 regression: blocked_set must stay correct under concurrent transitions.
/// 8 threads racing to insert (clean or blocked) the same IP — final set
/// state must match the actual `is_blocked()` state of the entry.
#[test]
fn blocked_set_concurrent_transitions_converge() {
    use std::sync::Arc;
    use std::thread;
    let store = Arc::new(Store::new(16));
    let ip: IpAddr = "10.8.8.8".parse().unwrap();
    let mut handles = vec![];
    for n in 0..8u32 {
        let s = store.clone();
        handles.push(thread::spawn(move || {
            for i in 0..100u32 {
                if (n + i) % 2 == 0 {
                    s.insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
                        .unwrap();
                } else {
                    s.insert(
                        ip,
                        Value::IpRecord(blocked_record(ip)),
                        None,
                        64 * 1024 * 1024,
                    )
                    .unwrap();
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // Final state of the entry decides whether the set contains ip.
    let entry_blocked = store.get(&ip).map(|v| v.is_blocked()).unwrap_or(false);
    let set_contains = !store.get_all_blocked_ips().is_empty();
    assert_eq!(
        entry_blocked, set_contains,
        "blocked_set membership must match the entry's is_blocked() state"
    );
    assert_store_invariants(&store);
}

/// P0 regression: merge_subnet_window read-modify-write must not lose
/// updates under concurrency. The old code did get().clone() → mutate
/// → insert, dropping the shard lock between read and write, so two
/// concurrent calls for the same subnet would both see the same
/// baseline and one update would be lost.
#[test]
fn merge_subnet_window_concurrent_no_lost_updates() {
    use std::sync::Arc;
    use std::thread;
    let store = Arc::new(Store::new(16));
    let sk: SubnetKey = 0x0a14_1e00_0000_0000_0000_0000_0000_0000;
    let net = IpNetwork::ipv4_subnet(std::net::Ipv4Addr::new(10, 20, 30, 0));
    // 16 threads each call merge_subnet_window with 1 event for the
    // same subnet, 100 ms apart so the window doesn't roll over.
    // Total expected: 16 events; old code frequently observed <16.
    let mut handles = vec![];
    for _ in 0..16 {
        let s = store.clone();
        handles.push(thread::spawn(move || {
            s.merge_subnet_window(sk, net, 1, None, 1_000_000_000);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let total = store.subnet_table().get(&sk).unwrap().total_rps;
    assert_eq!(total, 16, "all 16 concurrent merges must be counted");
}

/// P1-6 regression: evict_expired must remove TTL-expired entries and
/// reclaim ram_bytes. Insert with TTL=1s, sleep, sweep, assert gone.
#[test]
fn evict_expired_removes_ttl_entries() {
    use std::net::Ipv4Addr;
    let store = Store::new(16);
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 99, 0, 1));
    let ram_lim = 64 * 1024 * 1024;
    store
        .insert(ip, Value::Counter(42), Some(1), ram_lim)
        .unwrap();
    assert_eq!(store.len(), 1, "entry present before expiry");
    assert!(store.ram_bytes() > 0, "ram_bytes non-zero after insert");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let evicted = store.evict_expired();
    assert_eq!(evicted, 1, "evict_expired must remove the expired entry");
    assert_eq!(store.len(), 0, "store empty after eviction");
    assert_eq!(store.ram_bytes(), 0, "ram_bytes zeroed after eviction");
    assert_store_invariants(&store);
}

/// IPv6 plan Task 5: the v6 gate leg (Task 2) reads `subnet_index`
/// cardinality — stale members would count as ghosts forever if any
/// eviction path skipped index cleanup. v6 must behave exactly like v4.
#[test]
fn v6_eviction_cleans_subnet_index() {
    let store = Store::new(16);
    let ram_lim = 64 * 1024 * 1024;
    let hosts: Vec<IpAddr> = (1..=3u16)
        .map(|o| IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, o)))
        .collect();
    let sk = subnet_key_u128(hosts[0]).unwrap();
    for h in &hosts {
        store
            .insert(*h, Value::Counter(1), Some(1), ram_lim)
            .unwrap();
        store.update_subnet_index(*h, Some(sk), false);
    }
    assert_eq!(store.subnet_member_count(sk), 3, "index seeded");
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(store.evict_expired(), 3);
    assert_eq!(
        store.subnet_member_count(sk),
        0,
        "evicted v6 hosts must leave zero gate-countable ghosts"
    );
}

/// P0 regression (round-4 Q3): `update_ip` mutating a live Blocked record
/// must NOT revert block_state — the merge_record get/insert race let a
/// stale Clean snapshot resurrect blocked attackers.
#[test]
fn update_ip_preserves_block_state() {
    use std::net::Ipv4Addr;
    let store = Store::new(16);
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 98, 0, 1));
    let ram_lim = 64 * 1024 * 1024;
    store
        .insert(ip, Value::IpRecord(blocked_record(ip)), None, ram_lim)
        .unwrap();
    let (n, stored) = store.update_ip(ip, blank_record(ip), ram_lim, |r| {
        r.request_count += 5;
        r.request_count
    });
    assert!(stored);
    assert_eq!(n, 6);
    match store.get(&ip) {
        Some(Value::IpRecord(r)) => {
            assert!(
                matches!(r.block_state, BlockState::Blocked { .. }),
                "update_ip clobbered block state"
            );
            assert_eq!(r.request_count, 6);
        }
        other => panic!("expected IpRecord, got {other:?}"),
    }
}

/// Vacant path: creates the record, bumps accounting; capacity-exceeded
/// path must refuse (stored=false) and leave ram_bytes untouched.
#[test]
fn update_ip_vacant_and_capacity() {
    use std::net::Ipv4Addr;
    let store = Store::new(16);
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 97, 0, 1));
    let big = 64 * 1024 * 1024;
    let (v, stored) = store.update_ip(ip, blank_record(ip), big, |r| {
        r.request_count = 42;
        r.request_count
    });
    assert!(stored);
    assert_eq!(v, 42);
    assert_eq!(store.len(), 1);
    assert!(store.ram_bytes() > 0);
    let ram_before = store.ram_bytes();
    // Tiny budget: net-new key refused, nothing created, no leak.
    let ip2: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 97, 0, 2));
    let (_, stored2) = store.update_ip(ip2, blank_record(ip2), 1, |_r| ());
    assert!(!stored2, "must refuse net-new when budget exhausted");
    assert_eq!(store.ram_bytes(), ram_before, "refused insert leaked bytes");
    assert_eq!(store.len(), 1);
}

/// Check all secondary indexes are consistent with the authoritative store state.
/// Panics with a diagnostic on the first divergence.
fn assert_store_invariants(store: &Store) {
    // 1. blocked_set must match actual blocked entries from store iter.
    let mut actual_blocked: Vec<IpAddr> = vec![];
    for e in store.inner.iter() {
        if e.value().value.is_blocked() {
            actual_blocked.push(*e.key());
        }
    }
    // Every IP in blocked_set must actually be blocked.
    for e in store.blocked_set.iter() {
        let ip = *e.key();
        assert!(
            actual_blocked.contains(&ip),
            "blocked_set has phantom IP {ip} (not actually blocked)"
        );
    }
    // Every actually-blocked IP must be in blocked_set.
    for e in store.inner.iter() {
        if e.value().value.is_blocked() {
            let ip = *e.key();
            assert!(
                store.blocked_set.get(&ip).is_some(),
                "blocked_set missing actually-blocked IP {ip}"
            );
        }
    }
    // 2. blocked_count must match blocked_set size (synchronized under load).
    let count = store.blocked_count.load(Ordering::Relaxed);
    let set_size = store.blocked_set.len();
    assert_eq!(
        count, set_size as u64,
        "blocked_count {count} != blocked_set.len {set_size}"
    );
    // 3. ttl_entries must match number of entries with expires_at.
    let mut ttl_actual = 0u64;
    for e in store.inner.iter() {
        if e.value().expires_at.is_some() {
            ttl_actual += 1;
        }
    }
    let ttl_book = store.ttl_entries.load(Ordering::Relaxed);
    assert_eq!(
        ttl_actual, ttl_book,
        "ttl_entries ({ttl_book}) != actual entries with expires_at ({ttl_actual})"
    );
}

/// Overloaded concurrency stress: 8 IPs, 8 threads, each doing 500
/// random block/unblock/remove cycles. Verifies all invariants hold
/// after convergence.
#[test]
fn stress_block_unblock_concurrent() {
    use std::sync::Arc;
    use std::thread;
    let store = Arc::new(Store::new(16));
    let ips: Vec<IpAddr> = (0..8u8)
        .map(|n| format!("10.9.{}.1", n).parse().unwrap())
        .collect();
    let mut handles = vec![];
    for t in 0..8u32 {
        let s = store.clone();
        let ips = ips.clone();
        handles.push(thread::spawn(move || {
            let mut seed = t * 113;
            for _ in 0..500u32 {
                let ip = ips[seed as usize % ips.len()];
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let op = (seed >> 16) % 3;
                match op {
                    0 => {
                        s.insert(
                            ip,
                            Value::IpRecord(blocked_record(ip)),
                            None,
                            64 * 1024 * 1024,
                        )
                        .unwrap_or(());
                    }
                    1 => {
                        s.insert(ip, Value::Counter(1), None, 64 * 1024 * 1024)
                            .unwrap_or(());
                    }
                    _ => {
                        s.remove(&ip);
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_store_invariants(&store);
    assert_eq!(
        store.blocked_count.load(Ordering::Relaxed),
        store.blocked_set.len() as u64,
        "blocked_count must converge to blocked_set.len after concurrent stress"
    );
}

fn blank_record(ip: IpAddr) -> IpRecord {
    IpRecord {
        ip,
        request_count: 0,
        ewma_rps: 0.0,
        cusum_s: 0.0,
        baseline_rps: 0.0,
        prev_sample_hot: false,
        sample_count: 0,
        relative_breach_streak: 0,
        pulse_samples_in_window: 0,
        pulse_window_start_ns: 0,
        first_seen_ns: 0,
        last_seen_ns: 0,
        bytes_in: 0,
        status_dist: [0; 5],
        proto_fingerprint: 0,
        threat_score: 0.0,
        block_state: BlockState::Clean,
    }
}

#[test]
fn threat_sample_queue_is_bounded() {
    // P1-9: the queue must not grow unbounded when the forecaster (its
    // only drainer) is stalled or disabled.
    let s = TrafficCounters::new();
    let ip: IpAddr = "10.98.0.1".parse().unwrap();
    let many: Vec<(IpAddr, f32)> = (0..3000u32).map(|i| (ip, i as f32 + 0.5)).collect();
    s.push_threat_samples(many);
    assert!(
        s.threat_sample.len() <= 1024,
        "queue exceeded bound: {}",
        s.threat_sample.len()
    );
}

/// Full-capacity boundary for the `update_ip` read-modify-write path.
/// Unlike `insert` (which returns a `Result`), the `Vacant` branch of
/// `update_ip` refuses a net-new key silently via `stored=false` when the
/// CAS gate denies it. Contract: an entry that EXACTLY fits the remaining
/// budget must be accepted, and the next net-new key must be refused with
/// the store left consistent (no partial entry, no accounting drift).
#[test]
fn update_ip_capacity_gate_accepts_exact_fit_refuses_overflow() {
    let store = Store::new(16);
    // Budget for exactly one entry: fixed-size IpRecord, heap 0 (Clean).
    let one_entry = std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>();
    let first: IpAddr = "10.200.0.1".parse().unwrap();
    let (out1, stored1) = store.update_ip(first, blank_record(first), one_entry, |_| ());
    assert!(stored1, "an entry that exactly fits must be stored");
    assert_eq!(
        store.ram_bytes(),
        one_entry,
        "accounting must match the exact-fit entry"
    );

    // The budget is now exactly full: a different net-new key must be
    // refused, with no partial state.
    let second: IpAddr = "10.200.0.2".parse().unwrap();
    let (out2, stored2) = store.update_ip(second, blank_record(second), one_entry, |_| ());
    assert!(!stored2, "a net-new key at a full budget must be refused");
    assert!(
        !store.inner().contains_key(&second),
        "refused update_ip must not leave a phantom entry"
    );
    assert_eq!(
        store.ram_bytes(),
        one_entry,
        "refused update_ip must not perturb accounting"
    );

    // Replacement of the existing key is NOT gated (limit only gates
    // net-new growth): the same key must still succeed at the full budget.
    let (out3, stored3) = store.update_ip(first, blank_record(first), one_entry, |_| ());
    assert!(stored3, "replacement of an existing key must not be gated");
    assert_eq!(store.ram_bytes(), one_entry);
}
