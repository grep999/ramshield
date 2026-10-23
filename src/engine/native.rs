//! Native L3/L4 observation on Linux.
//!
//! The production path uses PACKET_MMAP/TPACKET_V3 rather than one recv() call
//! per packet. TPACKET_V3 exposes block-level polling and a mmap'd RX ring; this
//! keeps syscall and allocation pressure bounded during floods. Multiple
//! capture workers share a PACKET_FANOUT_HASH group so a hot flow is handled by
//! one worker while unrelated flows can use other CPUs.
//!
//! This is observation, not inline enforcement. XDP/netfilter remain the
//! enforcement layers.
use crossbeam_channel::Sender;
use ramshield_metrics::Metrics;
use ramshield_types::{ConnectionEvent, IpNetwork};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

#[cfg(target_os = "linux")]
const TPACKET_V3: i32 = 2;
#[cfg(target_os = "linux")]
const PACKET_VERSION: i32 = 10;
#[cfg(target_os = "linux")]
const PACKET_RX_RING: i32 = 5;
#[cfg(target_os = "linux")]
const PACKET_FANOUT: i32 = 18;
#[cfg(target_os = "linux")]
const PACKET_IGNORE_OUTGOING: i32 = 23;
#[cfg(target_os = "linux")]
const PACKET_FANOUT_HASH: u16 = 0;
#[cfg(target_os = "linux")]
const TP_STATUS_KERNEL: u32 = 0;
#[cfg(target_os = "linux")]
const TP_STATUS_USER: u32 = 1;
#[cfg(target_os = "linux")]
const TPACKET_ALIGNMENT: usize = 16;

#[cfg(target_os = "linux")]
#[repr(C)]
struct TpacketReq3 {
    tp_block_size: u32,
    tp_block_nr: u32,
    tp_frame_size: u32,
    tp_frame_nr: u32,
    tp_retire_blk_tov: u32,
    tp_sizeof_priv: u32,
    tp_feature_req_word: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct TpacketBdTs {
    ts_sec: u32,
    ts_nsec: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct TpacketHdrV1 {
    block_status: u32,
    num_pkts: u32,
    offset_to_first_pkt: u32,
    blk_len: u32,
    seq_num: u64,
    ts_first_pkt: TpacketBdTs,
    ts_last_pkt: TpacketBdTs,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct TpacketBlockDesc {
    version: u32,
    offset_to_priv: u32,
    hdr: TpacketHdrV1,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Tpacket3Hdr {
    tp_next_offset: u32,
    tp_sec: u32,
    tp_nsec: u32,
    tp_snaplen: u32,
    tp_len: u32,
    tp_status: u32,
    tp_mac: u16,
    tp_net: u16,
    tp_rxhash: u32,
    tp_vlan_tci: u32,
    tp_vlan_tpid: u16,
    tp_padding: u16,
    tp_padding2: [u8; 8],
}

/// How many capture attempts a worker slot makes before it gives up.
///
/// Capture setup failures are overwhelmingly persistent - a bad
/// interface name, a missing `CAP_NET_RAW`, an `mmap` the kernel
/// refuses. A persistent fault must not spin, but a genuinely transient
/// one (an interface flap, `ENOMEM` under pressure) clears within a
/// second or two. Three attempts a second apart covers the transient
/// case and fails fast on the persistent one.
pub(super) const MAX_CAPTURE_ATTEMPTS: u32 = 3;

/// The pause between two capture attempts of the same worker slot.
pub(super) const RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);

/// What a worker slot did before it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlotOutcome {
    /// How many capture attempts were made.
    pub attempts: u32,
    /// How many of them failed.
    pub failures: u32,
    /// Whether the daemon's shutdown flag stopped the slot.
    pub stopped_by_shutdown: bool,
}

/// Run one capture worker slot: at most `attempts_limit` capture
/// attempts, `backoff` apart, stopping early when the daemon shuts down.
///
/// The capture attempt is a parameter so the retry policy can be tested
/// without a NIC, a capability or a race.
pub(super) fn capture_slot(
    attempts_limit: u32,
    backoff: std::time::Duration,
    shutdown: &AtomicBool,
    mut attempt: impl FnMut(u32) -> std::io::Result<()>,
) -> SlotOutcome {
    let mut outcome = SlotOutcome {
        attempts: 0,
        failures: 0,
        stopped_by_shutdown: false,
    };
    while outcome.attempts < attempts_limit {
        if shutdown.load(Ordering::Acquire) {
            outcome.stopped_by_shutdown = true;
            break;
        }
        outcome.attempts += 1;
        match attempt(outcome.attempts) {
            // A clean return means the capture loop saw the shutdown
            // flag. The slot is done.
            Ok(()) => break,
            Err(err) => {
                outcome.failures += 1;
                if outcome.attempts >= attempts_limit {
                    error!(
                        attempts = outcome.attempts,
                        error = %err,
                        "native capture worker failed; giving up"
                    );
                    break;
                }
                // Shutdown wins over a retry: a slot must not re-open an
                // AF_PACKET socket after the daemon was asked to stop.
                if shutdown.load(Ordering::Acquire) {
                    outcome.stopped_by_shutdown = true;
                    break;
                }
                warn!(
                    attempts = outcome.attempts,
                    error = %err,
                    "native capture worker attempt failed; retrying"
                );
                std::thread::sleep(backoff);
            }
        }
    }
    outcome
}

/// Join the capture worker threads, reporting the ones that panicked.
///
/// Returns how many workers panicked. The old code dropped the join
/// result with `let _ = handle.join()`, so a worker that panicked was
/// indistinguishable from one that exited cleanly.
pub(super) fn join_workers(handles: Vec<std::thread::JoinHandle<()>>) -> usize {
    let mut panicked = 0usize;
    for handle in handles {
        if handle.join().is_err() {
            panicked += 1;
            error!("native capture worker panicked");
        }
    }
    panicked
}

/// Start the capture worker slots.
///
/// Returns the handles of the workers that started, how many started and
/// how many failed to start. The spawner is a parameter so a partial or
/// total startup failure can be produced deterministically, without a
/// NIC and without a race.
pub(super) fn start_workers<F>(
    expected: usize,
    mut spawn_one: F,
) -> (Vec<std::thread::JoinHandle<()>>, usize, usize)
where
    F: FnMut(usize) -> std::io::Result<std::thread::JoinHandle<()>>,
{
    let mut handles = Vec::with_capacity(expected);
    let mut started = 0usize;
    let mut failed = 0usize;
    for worker in 0..expected {
        match spawn_one(worker) {
            Ok(handle) => {
                started += 1;
                handles.push(handle);
            }
            Err(err) => {
                failed += 1;
                error!(
                    worker,
                    error = %err,
                    "native capture worker did not spawn; running with fewer workers"
                );
            }
        }
    }
    (handles, started, failed)
}

pub fn spawn(
    interface: String,
    max_events_per_sec: u64,
    trusted_overlay_cidrs: Vec<IpNetwork>,
    tx: Sender<ConnectionEvent>,
    shutdown: Arc<AtomicBool>,
    metrics: Arc<Metrics>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    #[cfg(target_os = "linux")]
    {
        preflight_linux(&interface)?;
        let workers = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(1);
        // The worker threads are created HERE, not inside the supervisor.
        // The supervisor used to create them, so their spawn results were
        // swallowed with `handles.into_iter().flatten()` and the daemon
        // reached ready state with zero capture workers running, without
        // a word about it. Creating them here means the outcome is known
        // before the daemon is told the capture path started.
        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        // Counts how many times the "downstream channel is closed"
        // error was actually EMITTED, shared by every worker so the
        // daemon logs it once. A closed channel rejects every
        // subsequent event, so an error log per event would be a
        // log flood; the drop counter records the volume and this
        // counter records that the error was raised exactly once.
        let disconnect_log_emissions = Arc::new(AtomicU64::new(0));
        metrics.set_native_capture_workers_expected(workers as u64);
        let (handles, started, failed) = start_workers(workers, |worker| {
            let iface = interface.clone();
            let txc = tx.clone();
            let stop = shutdown.clone();
            let trusted = trusted_overlay_cidrs.clone();
            let b = budget.clone();
            let e = epoch.clone();
            let base = epoch_base;
            // Clone outside the `move` closure: the closure takes
            // ownership of what it captures, so cloning inside it
            // would move the handle on the first iteration.
            let m = metrics.clone();
            let dl = disconnect_log_emissions.clone();
            let mc = metrics.clone();
            std::thread::Builder::new()
                .name(format!("rs-native-{worker}"))
                .spawn(move || {
                    // The slot is alive from here to the end of the
                    // thread, including between two capture attempts: a
                    // retry must not read as "capture gone".
                    mc.inc_native_capture_workers_running();
                    let outcome =
                        capture_slot(MAX_CAPTURE_ATTEMPTS, RETRY_BACKOFF, &stop, |_attempt| {
                            let result = run_linux(
                                iface.clone(),
                                max_events_per_sec,
                                trusted.clone(),
                                txc.clone(),
                                stop.clone(),
                                b.clone(),
                                e.clone(),
                                base,
                                worker,
                                m.clone(),
                                dl.clone(),
                            );
                            if result.is_err() {
                                m.inc_native_capture_worker_failure();
                            }
                            result
                        });
                    mc.dec_native_capture_workers_running();
                    if outcome.failures > 0 {
                        error!(
                            worker,
                            attempts = outcome.attempts,
                            failures = outcome.failures,
                            "native capture worker stopped"
                        );
                    }
                })
        });
        metrics.set_native_capture_workers_started(started as u64);
        if started == 0 {
            return Err(std::io::Error::other(format!(
                "native capture: no worker thread could be started ({failed} of {workers} failed)"
            )));
        }
        // The supervisor only joins now. Every worker is accounted for,
        // and a worker that panicked is reported instead of dropped.
        std::thread::Builder::new()
            .name("rs-native-ingest-supervisor".into())
            .spawn(move || {
                join_workers(handles);
            })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            interface,
            max_events_per_sec,
            trusted_overlay_cidrs,
            tx,
            shutdown,
            metrics,
        );
        Err(std::io::Error::other(
            "native_ingest requires Linux AF_PACKET",
        ))
    }
}

#[cfg(target_os = "linux")]
fn preflight_linux(interface: &str) -> std::io::Result<()> {
    let ifname = std::ffi::CString::new(interface)
        .map_err(|_| std::io::Error::other("interface contains NUL"))?;
    // SAFETY: ifname is a NUL-terminated CString; if_nametoindex only reads it.
    let ifindex = unsafe { libc::if_nametoindex(ifname.as_ptr()) };
    if ifindex == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = unsafe {
        libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW | libc::SOCK_NONBLOCK,
            (libc::ETH_P_ALL as u16).to_be() as i32,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| {
        let version = TPACKET_V3;
        set_sockopt(fd, libc::SOL_PACKET, PACKET_VERSION, &version)?;
        let ignore_outgoing: i32 = 1;
        set_sockopt(
            fd,
            libc::SOL_PACKET,
            PACKET_IGNORE_OUTGOING,
            &ignore_outgoing,
        )?;
        let mut addr: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
        addr.sll_family = libc::AF_PACKET as u16;
        addr.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
        addr.sll_ifindex = ifindex as i32;
        let rc = unsafe {
            libc::bind(
                fd,
                (&addr as *const libc::sockaddr_ll).cast(),
                std::mem::size_of::<libc::sockaddr_ll>() as u32,
            )
        };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })();
    unsafe {
        libc::close(fd);
    }
    result
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)] // Worker-owned state is intentionally explicit.
fn run_linux(
    interface: String,
    max_eps: u64,
    trusted_overlay_cidrs: Vec<IpNetwork>,
    tx: Sender<ConnectionEvent>,
    shutdown: Arc<AtomicBool>,
    budget: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
    epoch_base: std::time::Instant,
    worker: usize,
    metrics: Arc<Metrics>,
    disconnect_log_emissions: Arc<AtomicU64>,
) -> std::io::Result<()> {
    // B02: Early validation before any unsafe mmap or pointer arithmetic.
    // 1. Interface exists and is usable.
    preflight_linux(&interface)?;
    // 2. Parameters are within acceptable ranges (block_size, block_nr).
    let block_size = 1usize << 20;
    let block_nr = 8usize;
    let frame_size = 2048usize;
    let frame_nr = block_size / frame_size * block_nr;
    if block_size == 0 || block_nr == 0 || frame_size == 0 || frame_nr == 0 {
        return Err(std::io::Error::other("invalid TPACKET_V3 parameters"));
    }
    // 3. Memory layout fits within a single process.
    let ring_len = block_size
        .checked_mul(block_nr)
        .ok_or_else(|| std::io::Error::other("TPACKET ring length overflow"))?;
    if ring_len > isize::MAX as usize {
        return Err(std::io::Error::other("TPACKET ring too large"));
    }
    // 4. Kernel descriptor validation (will be re-checked per-block).
    // No unsafe mmap yet.
    // Proceed to socket creation, bind, and ring allocation.
    let fd = unsafe {
        libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW | libc::SOCK_NONBLOCK,
            (libc::ETH_P_ALL as u16).to_be() as i32,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = run_socket(
        fd,
        &interface,
        max_eps,
        &trusted_overlay_cidrs,
        &tx,
        &shutdown,
        &budget,
        &epoch,
        epoch_base,
        worker,
        &metrics,
        &disconnect_log_emissions,
    );
    unsafe {
        libc::close(fd);
    }
    result
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)] // Socket/ring resources stay explicit at this boundary.
fn run_socket(
    fd: i32,
    interface: &str,
    max_eps: u64,
    trusted_overlay_cidrs: &[IpNetwork],
    tx: &Sender<ConnectionEvent>,
    shutdown: &Arc<AtomicBool>,
    budget: &Arc<AtomicU64>,
    epoch: &Arc<AtomicU64>,
    epoch_base: std::time::Instant,
    worker: usize,
    metrics: &Metrics,
    disconnect_log_emissions: &AtomicU64,
) -> std::io::Result<()> {
    // B02: After socket creation, before mmap, validate interface index and bind.
    let ifname = std::ffi::CString::new(interface)
        .map_err(|_| std::io::Error::other("interface contains NUL"))?;
    // SAFETY: ifname is a NUL-terminated CString; if_nametoindex only reads it.
    let ifindex = unsafe { libc::if_nametoindex(ifname.as_ptr()) };
    if ifindex == 0 {
        return Err(std::io::Error::last_os_error());
    }

    // B02: Set TPACKET version and ignore outgoing. Reject early if syscalls fail.
    let version = TPACKET_V3;
    set_sockopt(fd, libc::SOL_PACKET, PACKET_VERSION, &version)?;
    let ignore_outgoing: i32 = 1;
    set_sockopt(
        fd,
        libc::SOL_PACKET,
        PACKET_IGNORE_OUTGOING,
        &ignore_outgoing,
    )?;

    // B02: Bind to the correct interface.
    let mut addr: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    addr.sll_family = libc::AF_PACKET as u16;
    addr.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
    addr.sll_ifindex = ifindex as i32;
    let rc = unsafe {
        libc::bind(
            fd,
            (&addr as *const libc::sockaddr_ll).cast(),
            std::mem::size_of::<libc::sockaddr_ll>() as u32,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }

    // B02: Fanout is safe here; group must be within u16 range.
    let group = (std::process::id() as u16).max(1);
    let fanout = ((PACKET_FANOUT_HASH as u32) << 16) | group as u32;
    set_sockopt(fd, libc::SOL_PACKET, PACKET_FANOUT, &fanout)?;

    // B02: Socket ring configuration and validation.
    let block_size = 1usize << 20;
    let block_nr = 8usize;
    let frame_size = 2048usize;
    let frame_nr = block_size / frame_size * block_nr;
    let req = TpacketReq3 {
        tp_block_size: block_size as u32,
        tp_block_nr: block_nr as u32,
        tp_frame_size: frame_size as u32,
        tp_frame_nr: frame_nr as u32,
        tp_retire_blk_tov: 64,
        tp_sizeof_priv: 0,
        tp_feature_req_word: 1,
    };
    set_sockopt(fd, libc::SOL_PACKET, PACKET_RX_RING, &req)?;
    let ring_len = block_size * block_nr;
    if ring_len == 0 || ring_len > isize::MAX as usize {
        return Err(std::io::Error::other("TPACKET ring length invalid"));
    }
    // B02: mmap the kernel ring; error immediately on failure.
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            ring_len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if map == libc::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }

    info!(iface=%interface, worker, workers=?std::thread::available_parallelism().ok().map(|n| n.get().min(8)), max_events_per_sec=max_eps, "native TPACKET_V3 ingest active");
    let mut block_idx = 0usize;
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    while !shutdown.load(Ordering::Acquire) {
        // B02: Safe block index calculation; guard against overflow.
        if block_idx >= block_nr {
            block_idx = 0;
        }
        // Validate the complete block range before reading any descriptor fields.
        if block_idx >= block_nr {
            break;
        }
        let block_offset = match block_idx.checked_mul(block_size) {
            Some(offset) if checked_range_end(offset, block_size, ring_len).is_some() => offset,
            _ => {
                warn!(
                    worker,
                    block_idx, block_size, ring_len, "rejecting invalid TPACKET_V3 block offset"
                );
                unsafe {
                    libc::munmap(map, ring_len);
                }
                return Err(std::io::Error::other(
                    "TPACKET block offset outside mapped ring",
                ));
            }
        };
        let block_ptr = unsafe { (map as *mut u8).add(block_offset) as *mut TpacketBlockDesc };
        // B02: Read block status safely and re-validate before proceeding.
        let status =
            unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*block_ptr).hdr.block_status)) };
        if status & TP_STATUS_USER == 0 {
            let rc = unsafe { libc::poll(&mut pollfd, 1, 50) };
            if rc < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    // SAFETY: map was returned by mmap with length ring_len here.
                    unsafe { libc::munmap(map, ring_len) };
                    return Err(error);
                }
            }
            continue;
        }
        std::sync::atomic::fence(Ordering::Acquire);
        // Validate kernel-provided lengths before using packet offsets.
        // SAFETY: block_ptr points into the mmap'd ring at a validated block_offset.
        let blk_len = match usize::try_from(unsafe { (*block_ptr).hdr.blk_len }) {
            Ok(v) => v,
            Err(_) => {
                warn!(worker, block_idx, "blk_len does not fit usize");
                break;
            }
        };
        if !valid_tpacket_block_len(blk_len, block_size) {
            warn!(
                worker,
                block_idx, blk_len, block_size, "rejecting invalid TPACKET_V3 block length"
            );
            std::sync::atomic::fence(Ordering::Release);
            unsafe {
                std::ptr::write_volatile(
                    std::ptr::addr_of_mut!((*block_ptr).hdr.block_status),
                    TP_STATUS_KERNEL,
                );
            }
            block_idx = (block_idx + 1) % block_nr;
            continue;
        }
        let num = unsafe { (*block_ptr).hdr.num_pkts };
        let first = unsafe { (*block_ptr).hdr.offset_to_first_pkt as usize };
        let mut off = first;
        for packet_idx in 0..num {
            let header_abs = match checked_tpacket_header_offset(
                block_idx, block_size, ring_len, blk_len, off,
            ) {
                Some(value) => value,
                None => {
                    warn!(
                        worker,
                        block_idx,
                        packet_idx,
                        off,
                        blk_len,
                        "rejecting invalid TPACKET_V3 packet-header offset"
                    );
                    break;
                }
            };
            // SAFETY: checked_tpacket_header_offset proves the complete header
            // lies in both the declared block and mapped ring before this read.
            let hdr = unsafe { (map as *const u8).add(header_abs) as *const Tpacket3Hdr };
            // SAFETY: hdr points at a valid Tpacket3Hdr inside the mapping.
            let snaplen = match usize::try_from(unsafe { (*hdr).tp_snaplen }) {
                Ok(v) => v,
                Err(_) => {
                    warn!(
                        worker,
                        block_idx, packet_idx, "tp_snaplen does not fit usize"
                    );
                    break;
                }
            };
            // SAFETY: same hdr validity as above.
            // `tp_mac` is u16, which fits in `usize` on every supported Linux
            // target. The range check below validates the value against the ring.
            let mac = unsafe { (*hdr).tp_mac as usize };
            let (_header_abs, packet_off) = match checked_tpacket_packet_offsets(
                block_idx, block_size, ring_len, blk_len, off, mac, snaplen,
            ) {
                Some(value) if mac <= frame_size && snaplen <= frame_size => value,
                _ => {
                    warn!(
                        worker,
                        block_idx,
                        packet_idx,
                        off,
                        mac,
                        snaplen,
                        blk_len,
                        "rejecting invalid TPACKET_V3 packet range"
                    );
                    break;
                }
            };
            // SAFETY: checked_tpacket_packet_offsets proves [packet_off,
            // packet_off + snaplen) lies in the current block and mapped ring.
            let packet =
                unsafe { std::slice::from_raw_parts((map as *const u8).add(packet_off), snaplen) };
            process_packet(
                packet,
                max_eps,
                trusted_overlay_cidrs,
                tx,
                budget,
                epoch,
                epoch_base,
                metrics,
                disconnect_log_emissions,
            );

            // SAFETY: hdr still points at the validated packet header in this block.
            let next = match usize::try_from(unsafe { (*hdr).tp_next_offset }) {
                Ok(v) => v,
                Err(_) => {
                    warn!(
                        worker,
                        block_idx, packet_idx, "tp_next_offset does not fit usize"
                    );
                    break;
                }
            };
            match checked_next_tpacket_offset(off, next, packet_idx + 1 == num, blk_len, block_size)
            {
                Ok(Some(next_off)) => off = next_off,
                Ok(None) => {}
                Err(reason) => {
                    warn!(worker, block_idx, packet_idx, off, next, %reason, "rejecting invalid TPACKET_V3 descriptor chain");
                    break;
                }
            }
        }
        std::sync::atomic::fence(Ordering::Release);
        unsafe {
            std::ptr::write_volatile(
                std::ptr::addr_of_mut!((*block_ptr).hdr.block_status),
                TP_STATUS_KERNEL,
            );
        }
        block_idx = (block_idx + 1) % block_nr;
    }
    // SAFETY: map was returned by mmap with length ring_len in this function.
    unsafe { libc::munmap(map, ring_len) };
    Ok(())
}

#[cfg(target_os = "linux")]
fn valid_tpacket_block_len(blk_len: usize, block_size: usize) -> bool {
    blk_len >= std::mem::size_of::<TpacketBlockDesc>() && blk_len <= block_size
}

#[cfg(target_os = "linux")]
fn checked_range_end(start: usize, len: usize, limit: usize) -> Option<usize> {
    let end = start.checked_add(len)?;
    (end <= limit).then_some(end)
}

#[cfg(target_os = "linux")]
fn checked_tpacket_header_offset(
    block_idx: usize,
    block_size: usize,
    ring_len: usize,
    blk_len: usize,
    off: usize,
) -> Option<usize> {
    let header_size = std::mem::size_of::<Tpacket3Hdr>();
    if !valid_tpacket_block_len(blk_len, block_size)
        || off < std::mem::size_of::<TpacketBlockDesc>()
        || !off.is_multiple_of(std::mem::align_of::<Tpacket3Hdr>())
    {
        return None;
    }
    checked_range_end(off, header_size, blk_len)?;
    checked_range_end(off, header_size, block_size)?;
    let block_base = block_idx.checked_mul(block_size)?;
    let block_end = checked_range_end(block_base, block_size, ring_len)?;
    let header_abs = block_base.checked_add(off)?;
    let header_end = checked_range_end(header_abs, header_size, ring_len)?;
    (header_abs < block_end && header_end <= block_end).then_some(header_abs)
}

#[cfg(target_os = "linux")]
fn checked_tpacket_packet_offsets(
    block_idx: usize,
    block_size: usize,
    ring_len: usize,
    blk_len: usize,
    off: usize,
    mac: usize,
    snaplen: usize,
) -> Option<(usize, usize)> {
    let header_abs = checked_tpacket_header_offset(block_idx, block_size, ring_len, blk_len, off)?;
    let packet_rel = off.checked_add(mac)?;
    let packet_rel_end = checked_range_end(packet_rel, snaplen, blk_len)?;
    checked_range_end(packet_rel, snaplen, block_size)?;
    let block_base = block_idx.checked_mul(block_size)?;
    let block_end = checked_range_end(block_base, block_size, ring_len)?;
    let packet_abs = block_base.checked_add(packet_rel)?;
    let packet_abs_end = checked_range_end(packet_abs, snaplen, ring_len)?;
    (packet_abs >= block_base && packet_abs_end <= block_end && packet_rel_end <= blk_len)
        .then_some((header_abs, packet_abs))
}

#[cfg(target_os = "linux")]
fn checked_next_tpacket_offset(
    off: usize,
    next: usize,
    is_last: bool,
    blk_len: usize,
    block_size: usize,
) -> Result<Option<usize>, &'static str> {
    if next == 0 {
        return if is_last {
            Ok(None)
        } else {
            Err("zero tp_next_offset before final packet")
        };
    }
    let aligned = next
        .checked_add(TPACKET_ALIGNMENT - 1)
        .ok_or("tp_next_offset alignment overflow")?
        & !(TPACKET_ALIGNMENT - 1);
    if aligned < std::mem::size_of::<Tpacket3Hdr>() {
        return Err("tp_next_offset does not advance past the current header");
    }
    let next_off = off
        .checked_add(aligned)
        .ok_or("tp_next_offset addition overflow")?;
    checked_range_end(next_off, std::mem::size_of::<Tpacket3Hdr>(), blk_len)
        .ok_or("next packet header exceeds declared block")?;
    checked_range_end(next_off, std::mem::size_of::<Tpacket3Hdr>(), block_size)
        .ok_or("next packet header exceeds block size")?;
    Ok(Some(next_off))
}

#[cfg(target_os = "linux")]
fn set_sockopt<T>(fd: i32, level: i32, name: i32, value: &T) -> std::io::Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (value as *const T).cast(),
            std::mem::size_of::<T>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)] // Producer state stays explicit at this boundary.
fn process_packet(
    packet: &[u8],
    max_eps: u64,
    trusted_overlay_cidrs: &[IpNetwork],
    tx: &Sender<ConnectionEvent>,
    budget: &Arc<AtomicU64>,
    epoch: &Arc<AtomicU64>,
    epoch_base: std::time::Instant,
    metrics: &Metrics,
    disconnect_log_emissions: &AtomicU64,
) {
    let second = epoch_base.elapsed().as_secs();
    let seen = epoch.load(Ordering::Relaxed);
    if seen != second
        && epoch
            .compare_exchange(seen, second, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        budget.store(0, Ordering::Relaxed);
    }
    let parsed = match parse_l3(packet) {
        Some(packet) => packet,
        None => return,
    };
    let ip = parsed.ip;
    let proto = parsed.protocol;
    let syn = parsed.syn;
    let overlay_identity = parsed.overlay_identity;
    if trusted_overlay_cidrs
        .iter()
        .any(|n| network_contains(n, ip))
        && !overlay_identity
    {
        // A shared cloud/LB/node address is not a safe enforcement identity.
        // Do not feed it into local detection; tenant-aware telemetry must
        // supply the real client identity separately.
        return;
    }
    let admitted = budget
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            let next = n.checked_add(1)?;
            if next <= max_eps { Some(next) } else { None }
        })
        .is_ok();
    if !admitted {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mut fp = proto as u32;
    if syn {
        fp |= 0x4000_0000;
    }
    if overlay_identity {
        fp |= 0x8000_0000;
    }
    let ev = ConnectionEvent {
        ip,
        timestamp_ns: ts,
        bytes: packet.len() as u64,
        status_code: 0,
        proto_fingerprint: fp,
        l7: None,
    };
    // The capture thread must never block on a full downstream queue, so the
    // send is non-blocking and a rejected event is dropped. That is the
    // right overload policy for a real-time ingest path — but only if the
    // loss is visible, and before this change the result was discarded
    // outright, so no counter moved and the loss was invisible both to the
    // operator and to the metrics export.
    match tx.try_send(ev) {
        Ok(()) => {}
        Err(crossbeam_channel::TrySendError::Full(_)) => {
            // Backpressure: the downstream queue is saturated.
            metrics.inc_native_ingest_drop(1);
        }
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
            // The downstream channel is closed. This is not backpressure —
            // the detection pipeline is gone — so it is counted separately
            // and logged. Every subsequent event is rejected too, so the
            // log is emitted once per daemon; the disconnect counter records
            // the volume.
            metrics.inc_native_ingest_drop(1);
            metrics.inc_native_ingest_disconnect(1);
            // compare_exchange, not a load-then-store: exactly one thread
            // may emit the error even if several workers notice the closed
            // channel at the same moment. The counter makes the one-shot
            // property observable to a test.
            if disconnect_log_emissions
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                error!(
                    "native capture: the downstream event channel is closed — observation events are being dropped"
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedPacket {
    ip: std::net::IpAddr,
    protocol: u8,
    syn: bool,
    fragmented: bool,
    overlay_identity: bool,
}

#[cfg(target_os = "linux")]
fn parse_l3(frame: &[u8]) -> Option<ParsedPacket> {
    if frame.len() < 14 {
        return None;
    }
    let mut off = 14usize;
    let mut eth = u16::from_be_bytes([frame[12], frame[13]]);
    for _ in 0..4 {
        if eth != 0x8100 && eth != 0x88a8 {
            break;
        }
        if frame.len() < off + 4 {
            return None;
        }
        eth = u16::from_be_bytes([frame[off + 2], frame[off + 3]]);
        off += 4;
    }
    match eth {
        0x0800 => parse_ipv4(frame, off),
        0x86dd => parse_ipv6(frame, off),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn parse_ipv4(frame: &[u8], off: usize) -> Option<ParsedPacket> {
    if frame.len() < off + 20 {
        return None;
    }
    let ihl = (frame[off] & 0x0f) as usize * 4;
    if ihl < 20 || frame.len() < off + ihl {
        return None;
    }
    let flags_frag = u16::from_be_bytes([frame[off + 6], frame[off + 7]]);
    let fragmented = (flags_frag & 0x1fff) != 0 || (flags_frag & 0x2000) != 0;
    let proto = frame[off + 9];
    let ip = std::net::Ipv4Addr::new(
        frame[off + 12],
        frame[off + 13],
        frame[off + 14],
        frame[off + 15],
    );
    let l4 = off + ihl;
    if fragmented {
        return Some(ParsedPacket {
            ip: std::net::IpAddr::V4(ip),
            protocol: proto,
            syn: false,
            fragmented: true,
            overlay_identity: false,
        });
    }

    // Native telemetry follows the same common overlay subset as XDP: IP-in-IP,
    // GRE without optional fields, VXLAN and Geneve with bounded option length.
    if proto == 4 && frame.len() >= l4 + 20 {
        let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
    }
    if proto == 47 && frame.len() >= l4 + 4 {
        let flags = u16::from_be_bytes([frame[l4], frame[l4 + 1]]);
        let inner_proto = u16::from_be_bytes([frame[l4 + 2], frame[l4 + 3]]);
        if (flags & 0x4F00) == 0 && (inner_proto == 0x0800 || inner_proto == 0x86dd) {
            let mut inner = l4 + 4;
            if (flags & 0x8000) != 0 {
                inner += 4;
            }
            if (flags & 0x2000) != 0 {
                inner += 4;
            }
            if (flags & 0x1000) != 0 {
                inner += 4;
            }
            if inner_proto == 0x0800 {
                let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
            }
            let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
        }
    }
    if proto == 17 && frame.len() >= l4 + 8 {
        let sport = u16::from_be_bytes([frame[l4], frame[l4 + 1]]);
        let dport = u16::from_be_bytes([frame[l4 + 2], frame[l4 + 3]]);
        if sport == 4789 || dport == 4789 {
            if frame.len() < l4 + 16 || frame[l4 + 8] & 0x08 == 0 {
                return None;
            }
            let inner_eth = l4 + 16;
            if frame.len() >= inner_eth + 14 {
                let inner_type = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if inner_type == 0x0800 {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
                if inner_type == 0x86dd {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
            }
        } else if sport == 6081 || dport == 6081 {
            if frame.len() < l4 + 8 {
                return None;
            }
            if frame[l4] >> 6 != 0 {
                return None;
            }
            let opt_len = (frame[l4] & 0x3f) as usize * 4;
            let inner_eth = l4 + 8 + opt_len;
            if frame.len() >= inner_eth + 14 {
                let inner_type = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if inner_type == 0x0800 {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
                if inner_type == 0x86dd {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
            }
        }
    }
    let syn = proto == 6
        && frame.len() >= l4 + 14
        && frame[l4 + 13] & 0x02 != 0
        && frame[l4 + 13] & 0x10 == 0;
    Some(ParsedPacket {
        ip: std::net::IpAddr::V4(ip),
        protocol: proto,
        syn,
        fragmented: false,
        overlay_identity: false,
    })
}

#[cfg(target_os = "linux")]
fn parse_ipv6(frame: &[u8], off: usize) -> Option<ParsedPacket> {
    if frame.len() < off + 40 {
        return None;
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&frame[off + 8..off + 24]);
    let mut next = frame[off + 6];
    let mut cursor = off + 40;
    let mut fragmented = false;
    for _ in 0..8 {
        match next {
            0 | 43 | 60 => {
                if frame.len() < cursor + 2 {
                    return None;
                }
                let len = (frame[cursor + 1] as usize + 1) * 8;
                next = frame[cursor];
                cursor = cursor.checked_add(len)?;
                if frame.len() < cursor {
                    return None;
                }
            }
            44 => {
                fragmented = true;
                if frame.len() < cursor + 8 {
                    return None;
                }
                next = frame[cursor];
                cursor += 8;
            }
            51 => {
                if frame.len() < cursor + 2 {
                    return None;
                }
                let len = (frame[cursor + 1] as usize + 2) * 4;
                next = frame[cursor];
                cursor += len;
                if frame.len() < cursor {
                    return None;
                }
            }
            50 => {
                return Some(ParsedPacket {
                    ip: std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)),
                    protocol: next,
                    syn: false,
                    fragmented: true,
                    overlay_identity: false,
                });
            }
            _ => break,
        }
    }
    if !fragmented && next == 41 && frame.len() >= cursor + 40 {
        let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
    }
    if !fragmented && next == 47 && frame.len() >= cursor + 4 {
        let flags = u16::from_be_bytes([frame[cursor], frame[cursor + 1]]);
        let inner_proto = u16::from_be_bytes([frame[cursor + 2], frame[cursor + 3]]);
        if (flags & 0x4F00) == 0 && inner_proto == 0x86dd && frame.len() >= cursor + 44 {
            let mut inner = cursor + 4;
            if (flags & 0x8000) != 0 {
                inner += 4;
            }
            if (flags & 0x2000) != 0 {
                inner += 4;
            }
            if (flags & 0x1000) != 0 {
                inner += 4;
            }
            let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
        }
    }
    if !fragmented && next == 17 && frame.len() >= cursor + 8 {
        let sport = u16::from_be_bytes([frame[cursor], frame[cursor + 1]]);
        let dport = u16::from_be_bytes([frame[cursor + 2], frame[cursor + 3]]);
        if sport == 4789 || dport == 4789 {
            if frame.len() < cursor + 16 || frame[cursor + 8] & 0x08 == 0 {
                return None;
            }
            let inner_eth = cursor + 16;
            if frame.len() >= inner_eth + 14 {
                let et = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if et == 0x86dd {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                } else if et == 0x0800 {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
            }
        } else if sport == 6081 || dport == 6081 {
            if frame.len() < cursor + 8 || frame[cursor] >> 6 != 0 {
                return None;
            }
            let opt_len = (frame[cursor] & 0x3f) as usize * 4;
            let inner_eth = cursor + 8 + opt_len;
            if frame.len() >= inner_eth + 14 {
                let et = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if et == 0x86dd {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                } else if et == 0x0800 {
                    let mut parsed = frag?;
                parsed.overlay_identity = true;
                return Some(parsed);
                }
            }
        }
    }
    let syn = !fragmented
        && next == 6
        && frame.len() >= cursor + 14
        && frame[cursor + 13] & 0x02 != 0
        && frame[cursor + 13] & 0x10 == 0;
    Some(ParsedPacket {
        ip: std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)),
        protocol: next,
        syn,
        fragmented,
        overlay_identity: false,
    })
}

fn network_contains(network: &IpNetwork, ip: std::net::IpAddr) -> bool {
    match (network.addr, ip) {
        (std::net::IpAddr::V4(net), std::net::IpAddr::V4(host)) => {
            let n = u32::from(net);
            let h = u32::from(host);
            let bits = network.prefix_len;
            if bits == 0 {
                true
            } else {
                (n & (!0u32 << (32 - bits))) == (h & (!0u32 << (32 - bits)))
            }
        }
        (std::net::IpAddr::V6(net), std::net::IpAddr::V6(host)) => {
            let n = u128::from(net);
            let h = u128::from(host);
            let bits = network.prefix_len;
            if bits == 0 {
                true
            } else {
                (n & (!0u128 << (128 - bits))) == (h & (!0u128 << (128 - bits)))
            }
        }
        _ => false,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tpacket_tests {
    use super::*;

    #[test]
    fn tpacket_constants_are_sane() {
        assert_eq!(TPACKET_V3, 2);
        assert_eq!(PACKET_RX_RING, 5);
    }

    #[test]
    fn rejects_block_length_outside_actual_block() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        assert!(!valid_tpacket_block_len(desc - 1, 128));
        assert!(!valid_tpacket_block_len(129, 128));
        assert!(valid_tpacket_block_len(128, 128));
    }

    #[test]
    fn rejects_header_at_or_beyond_block_end() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        let hdr = std::mem::size_of::<Tpacket3Hdr>();
        assert!(checked_tpacket_header_offset(0, 128, 128, 128, 128).is_none());
        assert!(checked_tpacket_header_offset(0, 128, 128, 128, 128 - hdr + 1).is_none());
        assert!(checked_tpacket_header_offset(0, 128, 128, 128, desc + 1).is_none());
        assert!(checked_tpacket_header_offset(0, 128, 128, 128, desc).is_some());
    }

    #[test]
    fn checked_range_rejects_offset_overflow() {
        assert!(checked_range_end(usize::MAX - 1, 8, usize::MAX).is_none());
        assert!(
            checked_next_tpacket_offset(usize::MAX - 8, 32, false, usize::MAX, usize::MAX).is_err()
        );
    }

    #[test]
    fn rejects_packet_range_past_block_and_mapped_ring() {
        // Header [48, 96) is valid, but packet [128, 129) crosses the block.
        assert!(checked_tpacket_packet_offsets(0, 128, 128, 128, 48, 80, 1).is_none());
        // On the final block, packet [255, 257) crosses the block boundary.
        assert!(checked_tpacket_packet_offsets(1, 128, 256, 128, 48, 79, 2).is_none());
        // A truncated mapping must also reject a block that extends past it.
        assert!(checked_tpacket_packet_offsets(1, 128, 255, 128, 48, 79, 1).is_none());
    }

    #[test]
    fn accepts_packet_ending_at_last_legal_byte() {
        // Header [48, 96), packet [127, 128): the final byte is in bounds.
        assert_eq!(
            checked_tpacket_packet_offsets(0, 128, 128, 128, 48, 79, 1),
            Some((48, 127))
        );
    }

    #[test]
    fn rejects_invalid_descriptor_chain_progress() {
        assert!(checked_next_tpacket_offset(48, 0, false, 128, 128).is_err());
        assert!(checked_next_tpacket_offset(48, 1, false, 128, 128).is_err());
        assert_eq!(checked_next_tpacket_offset(48, 0, true, 128, 128), Ok(None));
        assert_eq!(
            checked_next_tpacket_offset(48, 48, false, 192, 192),
            Ok(Some(96))
        );
    }

    /// Property-style sweep: blk_len must sit in [desc_size, block_size].
    #[test]
    fn property_block_len_all_boundaries() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        for block_size in [desc, 64, 128, 256, 4096, 1 << 16] {
            assert!(!valid_tpacket_block_len(0, block_size));
            if desc > 0 {
                assert!(!valid_tpacket_block_len(desc - 1, block_size));
            }
            assert!(valid_tpacket_block_len(desc, block_size));
            assert!(valid_tpacket_block_len(block_size, block_size));
            assert!(!valid_tpacket_block_len(
                block_size.saturating_add(1),
                block_size
            ));
            assert!(!valid_tpacket_block_len(usize::MAX, block_size));
        }
    }

    /// Header offsets near desc, near blk_len, and across multi-block rings.
    #[test]
    fn property_header_offset_edges_and_ring() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        let hdr = std::mem::size_of::<Tpacket3Hdr>();
        let block_size = 256usize;
        let blocks = 4usize;
        let ring_len = block_size * blocks;
        let blk_len = block_size;

        // Offsets below the block descriptor are illegal.
        for off in 0..desc {
            assert!(
                checked_tpacket_header_offset(0, block_size, ring_len, blk_len, off).is_none(),
                "off={off} must be rejected"
            );
        }
        // First legal header offset.
        assert!(checked_tpacket_header_offset(0, block_size, ring_len, blk_len, desc).is_some());

        // Header that would end past blk_len / block_size.
        let last_ok = blk_len - hdr;
        assert!(checked_tpacket_header_offset(0, block_size, ring_len, blk_len, last_ok).is_some());
        assert!(
            checked_tpacket_header_offset(0, block_size, ring_len, blk_len, last_ok + 1).is_none()
        );

        // Final block in a full mapping is OK; one byte short mapping is not.
        let last_block = blocks - 1;
        assert!(
            checked_tpacket_header_offset(last_block, block_size, ring_len, blk_len, desc)
                .is_some()
        );
        assert!(
            checked_tpacket_header_offset(last_block, block_size, ring_len - 1, blk_len, desc)
                .is_none()
        );

        // block_idx * block_size overflow / out of ring.
        assert!(
            checked_tpacket_header_offset(blocks, block_size, ring_len, blk_len, desc).is_none()
        );
        assert!(
            checked_tpacket_header_offset(usize::MAX / 2, block_size, ring_len, blk_len, desc)
                .is_none()
        );
    }

    /// Packet (mac, snaplen) combinations at block and ring edges.
    #[test]
    fn property_packet_mac_snaplen_edges() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        let block_size = 256usize;
        let ring_len = block_size * 2;
        let blk_len = block_size;
        let off = desc;

        // Empty packet is fine if the header itself is valid.
        assert!(
            checked_tpacket_packet_offsets(0, block_size, ring_len, blk_len, off, 0, 0).is_some()
        );

        // Packet that ends exactly at blk_len.
        let max_payload = blk_len - off;
        assert!(
            checked_tpacket_packet_offsets(0, block_size, ring_len, blk_len, off, 0, max_payload)
                .is_some()
        );
        assert!(
            checked_tpacket_packet_offsets(
                0,
                block_size,
                ring_len,
                blk_len,
                off,
                0,
                max_payload + 1
            )
            .is_none()
        );

        // mac + snaplen overflow.
        assert!(
            checked_tpacket_packet_offsets(0, block_size, ring_len, blk_len, off, usize::MAX, 1)
                .is_none()
        );
        assert!(
            checked_tpacket_packet_offsets(0, block_size, ring_len, blk_len, off, 1, usize::MAX)
                .is_none()
        );

        // Second block: packet may not cross into unmapped tail.
        assert!(
            checked_tpacket_packet_offsets(1, block_size, ring_len, blk_len, off, 0, max_payload)
                .is_some()
        );
        assert!(
            checked_tpacket_packet_offsets(
                1,
                block_size,
                ring_len - 1,
                blk_len,
                off,
                0,
                max_payload
            )
            .is_none()
        );
    }

    /// Descriptor chain must advance, stay aligned, and remain inside the block.
    #[test]
    fn property_descriptor_chain_progress_and_alignment() {
        let hdr = std::mem::size_of::<Tpacket3Hdr>();
        let block_size = 512usize;
        let blk_len = block_size;
        let off = 48usize;

        // Zero next only legal on last packet.
        assert!(checked_next_tpacket_offset(off, 0, false, blk_len, block_size).is_err());
        assert_eq!(
            checked_next_tpacket_offset(off, 0, true, blk_len, block_size),
            Ok(None)
        );

        // next whose *aligned* stride is still shorter than a header.
        for next in 1..hdr {
            let aligned = (next + TPACKET_ALIGNMENT - 1) & !(TPACKET_ALIGNMENT - 1);
            if aligned < hdr {
                assert!(
                    checked_next_tpacket_offset(off, next, false, blk_len, block_size).is_err(),
                    "next={next} aligned={aligned}"
                );
            }
        }

        // Aligned step that lands a full header inside the block.
        let step = hdr.div_ceil(TPACKET_ALIGNMENT) * TPACKET_ALIGNMENT;
        assert!(step >= hdr);
        let next_off = checked_next_tpacket_offset(off, step, false, blk_len, block_size)
            .expect("aligned step");
        assert_eq!(next_off, Some(off + step));

        // Step that would put the next header past blk_len.
        let too_far = blk_len - off - hdr + 1;
        if too_far >= hdr {
            assert!(checked_next_tpacket_offset(off, too_far, false, blk_len, block_size).is_err());
        }

        // Addition overflow on off + aligned next.
        assert!(
            checked_next_tpacket_offset(usize::MAX - 8, 32, false, usize::MAX, usize::MAX).is_err()
        );
    }

    /// Deterministic pseudo-fuzz over mixed malformed inputs (no external fuzzer).
    #[test]
    fn property_mixed_malformed_inputs_never_panic() {
        let desc = std::mem::size_of::<TpacketBlockDesc>();
        let hdr = std::mem::size_of::<Tpacket3Hdr>();
        let mut seed = 0xC0FFEE_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x144C_BC75);
            seed as usize
        };

        for _ in 0..2_000 {
            let block_size = (next() % 4096).max(desc);
            let blocks = (next() % 8).max(1);
            let ring_len = block_size.saturating_mul(blocks);
            let blk_len = next() % (block_size.saturating_add(64));
            let block_idx = next() % (blocks + 2);
            let off = next() % (block_size.saturating_add(32));
            let mac = next() % 256;
            let snaplen = next() % 512;
            let next_step = next() % 256;

            let _ = valid_tpacket_block_len(blk_len, block_size);
            let _ = checked_tpacket_header_offset(block_idx, block_size, ring_len, blk_len, off);
            let _ = checked_tpacket_packet_offsets(
                block_idx, block_size, ring_len, blk_len, off, mac, snaplen,
            );
            let _ = checked_next_tpacket_offset(
                off,
                next_step,
                next() % 2 == 0,
                blk_len.max(hdr),
                block_size.max(hdr),
            );
            let _ = checked_range_end(next(), next() % 128, next());
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn eth(etype: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 14];
        f[12..14].copy_from_slice(&etype.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn parses_ipv4_tcp_syn() {
        let mut ip = vec![0u8; 40];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&(40u16).to_be_bytes());
        ip[9] = 6;
        ip[12..16].copy_from_slice(&[198, 51, 100, 7]);
        ip[16..20].copy_from_slice(&[192, 0, 2, 10]);
        ip[20..22].copy_from_slice(&(40000u16).to_be_bytes());
        ip[22..24].copy_from_slice(&(443u16).to_be_bytes());
        ip[32] = 0x50;
        ip[33] = 0x02;
        let frame = eth(0x0800, &ip);
        let parsed = parse_l3(&frame).unwrap();
        let src = parsed.ip;
        let proto = parsed.protocol;
        let syn = parsed.syn;
        let fragmented = parsed.fragmented;
        let overlay = parsed.overlay_identity;
        assert_eq!(src, "198.51.100.7".parse::<std::net::IpAddr>().unwrap());
        assert_eq!(proto, 6);
        assert!(syn);
        assert!(!fragmented);
        assert!(!overlay);
    }

    /// Manual synthetic baseline for the native packet-to-event path. This
    /// avoids AF_PACKET privileges and measures parsing, admission, event
    /// construction, and queue insertion; it does not measure kernel capture
    /// or TPACKET ring traversal.
    #[test]
    #[ignore = "manual performance baseline; run with --ignored --nocapture"]
    fn native_packet_to_event_baseline() {
        const SAMPLES: usize = 5;
        const PACKETS_PER_SAMPLE: usize = 100_000;

        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let mut ip = vec![0u8; 40];
            ip[0] = 0x45;
            ip[2..4].copy_from_slice(&(40u16).to_be_bytes());
            ip[9] = 6;
            ip[12..16].copy_from_slice(&[198, 51, 100, 7]);
            ip[16..20].copy_from_slice(&[192, 0, 2, 10]);
            ip[20..22].copy_from_slice(&(40000u16).to_be_bytes());
            ip[22..24].copy_from_slice(&(443u16).to_be_bytes());
            ip[32] = 0x50;
            ip[33] = 0x02;
            let frame = eth(0x0800, &ip);
            let (tx, rx) = crossbeam_channel::bounded(PACKETS_PER_SAMPLE);
            let budget = Arc::new(AtomicU64::new(0));
            let epoch = Arc::new(AtomicU64::new(0));
            let epoch_base = std::time::Instant::now();
            let metrics = Arc::new(Metrics::new());
            let disconnect_log_emissions = Arc::new(AtomicU64::new(0));

            let start = std::time::Instant::now();
            for _ in 0..PACKETS_PER_SAMPLE {
                process_packet(
                    &frame,
                    PACKETS_PER_SAMPLE as u64,
                    &[],
                    &tx,
                    &budget,
                    &epoch,
                    epoch_base,
                    &metrics,
                    &disconnect_log_emissions,
                );
            }
            let elapsed = start.elapsed();
            assert_eq!(rx.len(), PACKETS_PER_SAMPLE);
            samples.push(elapsed.as_nanos() as f64 / PACKETS_PER_SAMPLE as f64);
        }

        samples.sort_by(f64::total_cmp);
        println!(
            "native_packet_to_event: median {:.1} ns/packet ({:.0} packets/s), samples={samples:?}",
            samples[SAMPLES / 2],
            1_000_000_000.0 / samples[SAMPLES / 2]
        );
    }

    #[test]
    fn rejects_ipv4_fragment_as_transport_identity() {
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        ip[9] = 17;
        ip[12..16].copy_from_slice(&[198, 51, 100, 8]);
        let frame = eth(0x0800, &ip);
        let parsed = parse_l3(&frame).unwrap();
        let syn = parsed.syn;
        let fragmented = parsed.fragmented;
        assert!(!syn);
        assert!(fragmented);
    }

    #[test]
    fn parses_ipv6_udp() {
        let mut ip = vec![0u8; 48];
        ip[0] = 0x60;
        ip[6] = 17;
        ip[7] = 64;
        ip[8..24].copy_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
        ip[24..40].copy_from_slice(&std::net::Ipv6Addr::UNSPECIFIED.octets());
        let frame = eth(0x86dd, &ip);
        let parsed = parse_l3(&frame).unwrap();
        let src = parsed.ip;
        let proto = parsed.protocol;
        let syn = parsed.syn;
        let fragmented = parsed.fragmented;
        let overlay = parsed.overlay_identity;
        assert_eq!(src, std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
        assert_eq!(proto, 17);
        assert!(!syn);
        assert!(!fragmented);
        assert!(!overlay);
    }

    /// A synthetic Ethernet+IPv4+TCP frame the native producer accepts.
    fn sample_frame() -> Vec<u8> {
        let mut ip = vec![0u8; 40];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&(40u16).to_be_bytes());
        ip[9] = 6;
        ip[12..16].copy_from_slice(&[198, 51, 100, 7]);
        ip[16..20].copy_from_slice(&[192, 0, 2, 10]);
        ip[20..22].copy_from_slice(&(40000u16).to_be_bytes());
        ip[22..24].copy_from_slice(&(443u16).to_be_bytes());
        ip[32] = 0x50;
        ip[33] = 0x02;
        eth(0x0800, &ip)
    }

    /// The native producer must not drop an observation event silently.
    ///
    /// The capture path sends without blocking, so a saturated or closed
    /// channel drops the event rather than stalling the capture thread.
    /// That is the right overload policy for a real-time ingest path — but
    /// only if it is *visible*. Before this change the `try_send` result was
    /// discarded outright, so no counter moved: the loss was invisible both
    /// to the operator and to the metrics export.
    #[test]
    fn native_ingest_counts_a_drop_when_the_channel_is_full() {
        let metrics = Metrics::new();
        let (tx, _rx) = crossbeam_channel::bounded(1);
        // Fill the channel so the next send is rejected as Full.
        tx.try_send(ConnectionEvent {
            ip: "198.51.100.7".parse().unwrap(),
            timestamp_ns: 0,
            bytes: 1,
            status_code: 0,
            proto_fingerprint: 0,
            l7: None,
        })
        .expect("seed the channel");

        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        let disconnect_log_emissions = AtomicU64::new(0);

        process_packet(
            &sample_frame(),
            1000,
            &[],
            &tx,
            &budget,
            &epoch,
            epoch_base,
            &metrics,
            &disconnect_log_emissions,
        );

        assert_eq!(
            metrics.native_ingest_drops.load(Ordering::Relaxed),
            1,
            "a dropped observation event must be counted"
        );
        assert_eq!(
            metrics
                .native_ingest_drops_disconnected
                .load(Ordering::Relaxed),
            0,
            "a full channel is not a disconnected channel"
        );
        assert_eq!(
            disconnect_log_emissions.load(Ordering::Relaxed),
            0,
            "a full channel is backpressure, not a dead pipeline — nothing to log"
        );
    }

    /// A **closed** downstream channel is a different failure from a full
    /// one: the detection pipeline is gone, not busy. It must be counted
    /// separately so an operator can tell the two apart, and it must be
    /// logged — but only once, because every subsequent event would also be
    /// rejected and a per-event log would be a flood.
    #[test]
    fn native_ingest_counts_a_disconnect_separately_from_a_full_channel() {
        let metrics = Metrics::new();
        let (tx, rx) = crossbeam_channel::bounded(1);
        drop(rx);

        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        let disconnect_log_emissions = AtomicU64::new(0);

        process_packet(
            &sample_frame(),
            1000,
            &[],
            &tx,
            &budget,
            &epoch,
            epoch_base,
            &metrics,
            &disconnect_log_emissions,
        );

        assert_eq!(
            metrics.native_ingest_drops.load(Ordering::Relaxed),
            1,
            "a dropped observation event must be counted"
        );
        assert_eq!(
            metrics
                .native_ingest_drops_disconnected
                .load(Ordering::Relaxed),
            1,
            "a closed channel must be counted as a disconnect, not as backpressure"
        );
        assert_eq!(
            disconnect_log_emissions.load(Ordering::Relaxed),
            1,
            "the first disconnect must be logged"
        );
    }

    /// The disconnect error must be logged exactly once, however many events
    /// are rejected afterwards — otherwise a dead pipeline becomes a log
    /// flood that hides everything else.
    #[test]
    fn native_ingest_logs_a_disconnect_only_once() {
        let metrics = Metrics::new();
        let (tx, rx) = crossbeam_channel::bounded(1);
        drop(rx);

        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        let disconnect_log_emissions = AtomicU64::new(0);

        for _ in 0..5 {
            process_packet(
                &sample_frame(),
                1000,
                &[],
                &tx,
                &budget,
                &epoch,
                epoch_base,
                &metrics,
                &disconnect_log_emissions,
            );
        }

        assert_eq!(
            metrics.native_ingest_drops.load(Ordering::Relaxed),
            5,
            "every rejected event must be counted"
        );
        assert_eq!(
            metrics
                .native_ingest_drops_disconnected
                .load(Ordering::Relaxed),
            5
        );
        assert_eq!(
            disconnect_log_emissions.load(Ordering::Relaxed),
            1,
            "the error must be emitted once, however many events are rejected"
        );
    }

    /// The success path must not move either counter: a delivered event is
    /// not a drop.
    #[test]
    fn native_ingest_does_not_count_a_delivered_event_as_a_drop() {
        let metrics = Metrics::new();
        let (tx, rx) = crossbeam_channel::bounded(1);

        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        let disconnect_log_emissions = AtomicU64::new(0);

        process_packet(
            &sample_frame(),
            1000,
            &[],
            &tx,
            &budget,
            &epoch,
            epoch_base,
            &metrics,
            &disconnect_log_emissions,
        );

        assert_eq!(rx.len(), 1, "the event must have been delivered");
        assert_eq!(
            metrics.native_ingest_drops.load(Ordering::Relaxed),
            0,
            "a delivered event is not a drop"
        );
        assert_eq!(
            metrics
                .native_ingest_drops_disconnected
                .load(Ordering::Relaxed),
            0
        );
        assert_eq!(disconnect_log_emissions.load(Ordering::Relaxed), 0);
    }

    /// The one-shot error must hold under contention: several workers may
    /// notice the closed channel at the same moment, and exactly one of
    /// them may emit the error.
    #[test]
    fn native_ingest_emits_the_disconnect_error_once_under_contention() {
        let metrics = Arc::new(Metrics::new());
        let (tx, rx) = crossbeam_channel::bounded(1);
        drop(rx);

        let budget = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let epoch_base = std::time::Instant::now();
        let disconnect_log_emissions = Arc::new(AtomicU64::new(0));

        let mut threads = Vec::new();
        for _ in 0..8 {
            let tx = tx.clone();
            let metrics = metrics.clone();
            let budget = budget.clone();
            let epoch = epoch.clone();
            let disconnect_log_emissions = disconnect_log_emissions.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    process_packet(
                        &sample_frame(),
                        1_000_000,
                        &[],
                        &tx,
                        &budget,
                        &epoch,
                        epoch_base,
                        &metrics,
                        &disconnect_log_emissions,
                    );
                }
            }));
        }
        for t in threads {
            t.join().expect("worker thread");
        }

        assert_eq!(
            metrics.native_ingest_drops.load(Ordering::Relaxed),
            800,
            "every rejected event must be counted"
        );
        assert_eq!(
            metrics
                .native_ingest_drops_disconnected
                .load(Ordering::Relaxed),
            800
        );
        assert_eq!(
            disconnect_log_emissions.load(Ordering::Relaxed),
            1,
            "exactly one thread may emit the error, however many notice the closed channel"
        );
    }
}
