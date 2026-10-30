//! #90 live native-capture integration test.
//!
//! Self-contained virtual environment: creates a veth pair and a peer network
//! namespace, starts the real `native::spawn` TPACKET_V3 path on the host veth,
//! floods ingress TCP SYNs from the peer, and checks the accounting.
//!
//! Needs root (CAP_NET_ADMIN + CAP_NET_RAW). Build unprivileged, run the test
//! binary privileged: `scripts/native_capture_netns.sh`.
#![cfg(target_os = "linux")]

use ramshield::engine::native;
use ramshield_metrics::Metrics;
use ramshield_types::ConnectionEvent;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const IP: &str = "/usr/sbin/ip";
const NS: &str = "rs-cap-n";
const HOST_VETH: &str = "rs-cap-h";
const NS_VETH: &str = "rs-cap-p";
const HOST_IP: &str = "10.99.0.1";
const NS_IP: &str = "10.99.0.2";
const SYNS: usize = 3000;

fn ip(args: &[&str]) -> bool {
    Command::new(IP)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ns_ip(args: &[&str]) -> bool {
    let mut full = vec!["netns", "exec", NS, IP];
    full.extend_from_slice(args);
    ip(&full)
}

struct Veth;

impl Veth {
    fn setup() -> Self {
        // Best-effort cleanup of a stale run.
        drop(Self);
        assert!(ip(&["netns", "add", NS]));
        assert!(ip(&[
            "link", "add", HOST_VETH, "type", "veth", "peer", "name", NS_VETH
        ]));
        assert!(ip(&["link", "set", NS_VETH, "netns", NS]));
        assert!(ip(&[
            "addr",
            "add",
            &format!("{HOST_IP}/30"),
            "dev",
            HOST_VETH
        ]));
        assert!(ip(&["link", "set", HOST_VETH, "up"]));
        assert!(ns_ip(&[
            "addr",
            "add",
            &format!("{NS_IP}/30"),
            "dev",
            NS_VETH
        ]));
        assert!(ns_ip(&["link", "set", NS_VETH, "up"]));
        assert!(ns_ip(&["link", "set", "lo", "up"]));
        Self
    }
}

impl Drop for Veth {
    fn drop(&mut self) {
        let _ = ip(&["netns", "del", NS]);
        let _ = ip(&["link", "del", HOST_VETH]);
    }
}

fn flood_syns() {
    // Each connect sends one SYN; the host has no listener so it answers RST.
    // The per-connect timeout bounds a dropped-SYN run.
    let script = format!(
        "for i in $(seq 1 {SYNS}); do timeout 0.1 bash -c 'echo > /dev/tcp/{HOST_IP}/9' 2>/dev/null; done"
    );
    let ok = ip(&["netns", "exec", NS, "bash", "-c", &script]);
    assert!(ok, "SYN flood failed");
}

#[test]
#[ignore = "needs root (CAP_NET_ADMIN + CAP_NET_RAW); use scripts/native_capture_netns.sh"]
fn live_tpacket_v3_capture_accounts_for_ingress_syns() {
    // SAFETY: geteuid has no preconditions.
    assert_eq!(unsafe { libc::geteuid() }, 0, "run as root");

    let _veth = Veth::setup();
    let metrics = Arc::new(Metrics::new());
    let (tx, rx) = crossbeam_channel::bounded::<ConnectionEvent>(1 << 16);
    let shutdown = Arc::new(AtomicBool::new(false));

    let handle = native::spawn(
        HOST_VETH.to_string(),
        1_000_000,
        Vec::new(),
        tx,
        shutdown.clone(),
        metrics.clone(),
    )
    .expect("native capture must start on the veth");

    // Let the capture workers open the socket and arm the ring.
    std::thread::sleep(Duration::from_millis(500));
    flood_syns();
    // Allow the ring to drain and one sampling interval to elapse.
    std::thread::sleep(Duration::from_millis(1500));

    shutdown.store(true, Ordering::Release);
    handle.join().expect("native supervisor must join cleanly");

    let emitted = metrics.native_ingest_emitted.load(Ordering::Relaxed);
    let drops = metrics.native_ingest_drops.load(Ordering::Relaxed);
    let parse = metrics.native_parse_rejects.load(Ordering::Relaxed);
    let overlay = metrics.native_overlay_skips.load(Ordering::Relaxed);
    let rate = metrics.native_rate_limit_skips.load(Ordering::Relaxed);
    let declared = metrics.native_frames_declared.load(Ordering::Relaxed);
    let walked = metrics.native_frames_walked.load(Ordering::Relaxed);
    let validated = metrics.native_frames_validated.load(Ordering::Relaxed);
    let socket_packets = metrics.native_socket_packets.load(Ordering::Relaxed);
    let socket_drops = metrics.native_socket_drops.load(Ordering::Relaxed);
    let queued = rx.try_iter().count() as u64;

    eprintln!(
        "socket_packets={socket_packets} socket_drops={socket_drops} \
         declared={declared} walked={walked} validated={validated} \
         emitted={emitted} queued={queued} drops={drops} parse={parse} \
         overlay={overlay} rate={rate}"
    );

    // Kernel delivered every SYN it captured to the ring (accumulated reads).
    assert!(
        socket_packets >= declared,
        "kernel reported fewer packets ({socket_packets}) than declared descriptors ({declared})"
    );
    // Well-formed kernel blocks: every declared descriptor is walked and
    // every walked descriptor yields a slice (SYNs parse).
    assert_eq!(walked, declared, "walker dropped declared descriptors");
    assert_eq!(validated, walked, "walked descriptors failed validation");
    // Every validated slice has exactly one terminal outcome.
    assert_eq!(
        validated,
        emitted + drops + parse + overlay + rate,
        "validated != sum of terminal outcomes"
    );
    assert!(emitted > 0, "no events emitted");
    assert_eq!(
        queued, emitted,
        "queue received a different count than emitted"
    );

    // The flood must be visible: allow for reruns and lost SYNs, but require
    // the capture path to have seen a substantial fraction.
    assert!(
        declared >= (SYNS as u64 / 2),
        "captured only {declared} of {SYNS} SYNs"
    );
}
