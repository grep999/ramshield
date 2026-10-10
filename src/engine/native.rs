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
use ramshield_types::{ConnectionEvent, IpNetwork};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

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

pub fn spawn(interface: String, max_events_per_sec: u64, trusted_overlay_cidrs: Vec<IpNetwork>, tx: Sender<ConnectionEvent>, shutdown: Arc<AtomicBool>) -> std::io::Result<std::thread::JoinHandle<()>> {
    #[cfg(target_os = "linux")]
    {
        preflight_linux(&interface)?;
        let workers = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(1);
        return std::thread::Builder::new().name("rs-native-ingest-supervisor".into()).spawn(move || {
            let budget = Arc::new(AtomicU64::new(0));
                let epoch = Arc::new(AtomicU64::new(0));
            let epoch_base = std::time::Instant::now();
            let mut handles = Vec::with_capacity(workers);
            for worker in 0..workers {
                let iface = interface.clone();
                let txc = tx.clone();
                let stop = shutdown.clone();
                let trusted = trusted_overlay_cidrs.clone();
                let b = budget.clone();
                let e = epoch.clone();
                let base = epoch_base;
                handles.push(std::thread::Builder::new().name(format!("rs-native-{worker}")).spawn(move || {
                    if let Err(err) = run_linux(iface, max_events_per_sec, trusted, txc, stop, b, e, base, worker) {
                        warn!(worker, error=%err, "native packet capture stopped");
                    }
                }));
            }
            for handle in handles.into_iter().flatten() { let _ = handle.join(); }
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface, max_events_per_sec, trusted_overlay_cidrs, tx, shutdown);
        Err(std::io::Error::other("native_ingest requires Linux AF_PACKET"))
    }
}

#[cfg(target_os = "linux")]
fn preflight_linux(interface: &str) -> std::io::Result<()> {
    let ifname = std::ffi::CString::new(interface)
        .map_err(|_| std::io::Error::other("interface contains NUL"))?;
    let ifindex = unsafe { libc::if_nametoindex(ifname.as_ptr()) };
    if ifindex == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW | libc::SOCK_NONBLOCK, (libc::ETH_P_ALL as u16).to_be() as i32) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| {
        let version = TPACKET_V3;
        set_sockopt(fd, libc::SOL_PACKET, PACKET_VERSION, &version)?;
        let ignore_outgoing: i32 = 1;
        set_sockopt(fd, libc::SOL_PACKET, PACKET_IGNORE_OUTGOING, &ignore_outgoing)?;
        let mut addr: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
        addr.sll_family = libc::AF_PACKET as u16;
        addr.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
        addr.sll_ifindex = ifindex as i32;
        let rc = unsafe { libc::bind(fd, (&addr as *const libc::sockaddr_ll).cast(), std::mem::size_of::<libc::sockaddr_ll>() as u32) };
        if rc < 0 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    })();
    unsafe { libc::close(fd); }
    result
}

#[cfg(target_os = "linux")]
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
    let ring_len = block_size * block_nr;
    if ring_len > isize::MAX as usize {
        return Err(std::io::Error::other("TPACKET ring too large"));
    }
    // 4. Kernel descriptor validation (will be re-checked per-block).
    // No unsafe mmap yet.
    // Proceed to socket creation, bind, and ring allocation.
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW | libc::SOCK_NONBLOCK, (libc::ETH_P_ALL as u16).to_be() as i32) };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    let result = run_socket(fd, &interface, max_eps, &trusted_overlay_cidrs, &tx, &shutdown, &budget, &epoch, epoch_base, worker);
    unsafe { libc::close(fd); }
    result
}

#[cfg(target_os = "linux")]
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
) -> std::io::Result<()> {
    // B02: After socket creation, before mmap, validate interface index and bind.
    let ifname = std::ffi::CString::new(interface).map_err(|_| std::io::Error::other("interface contains NUL"))?;
    let ifindex = unsafe { libc::if_nametoindex(ifname.as_ptr()) };
    if ifindex == 0 { return Err(std::io::Error::last_os_error()); }

    // B02: Set TPACKET version and ignore outgoing. Reject early if syscalls fail.
    let version = TPACKET_V3;
    set_sockopt(fd, libc::SOL_PACKET, PACKET_VERSION, &version)?;
    let ignore_outgoing: i32 = 1;
    set_sockopt(fd, libc::SOL_PACKET, PACKET_IGNORE_OUTGOING, &ignore_outgoing)?;

    // B02: Bind to the correct interface.
    let mut addr: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    addr.sll_family = libc::AF_PACKET as u16;
    addr.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
    addr.sll_ifindex = ifindex as i32;
    let rc = unsafe { libc::bind(fd, (&addr as *const libc::sockaddr_ll).cast(), std::mem::size_of::<libc::sockaddr_ll>() as u32) };
    if rc < 0 { return Err(std::io::Error::last_os_error()); }

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
    let map = unsafe { libc::mmap(std::ptr::null_mut(), ring_len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
    if map == libc::MAP_FAILED { return Err(std::io::Error::last_os_error()); }

    info!(iface=%interface, worker, workers=?std::thread::available_parallelism().ok().map(|n| n.get().min(8)), max_events_per_sec=max_eps, "native TPACKET_V3 ingest active");
    let mut block_idx = 0usize;
    let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    while !shutdown.load(Ordering::Acquire) {
        // B02: Safe block index calculation; guard against overflow.
        if block_idx >= block_nr { block_idx = 0; }
        // B02: Validate block descriptor range before any unsafe read.
        if block_idx >= block_nr { break; }
        let block_ptr = unsafe { (map as *mut u8).add(block_idx * block_size) as *mut TpacketBlockDesc };
        // B02: Read block status safely and re-validate before proceeding.
        let status = unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*block_ptr).hdr.block_status)) };
        if status & TP_STATUS_USER == 0 {
            let rc = unsafe { libc::poll(&mut pollfd, 1, 50) };
            if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted { break; }
            continue;
        }
        std::sync::atomic::fence(Ordering::Acquire);
        // Validate kernel-provided lengths before using packet offsets.
        let blk_len = unsafe { (*block_ptr).hdr.blk_len } as usize;
        if !valid_tpacket_block_len(blk_len, block_size) {
            warn!(worker, block_idx, blk_len, block_size, "rejecting invalid TPACKET_V3 block length");
            std::sync::atomic::fence(Ordering::Release);
            unsafe { std::ptr::write_volatile(std::ptr::addr_of_mut!((*block_ptr).hdr.block_status), TP_STATUS_KERNEL); }
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
                    warn!(worker, block_idx, packet_idx, off, blk_len, "rejecting invalid TPACKET_V3 packet-header offset");
                    break;
                }
            };
            // SAFETY: checked_tpacket_header_offset proves the complete header
            // lies in both the declared block and mapped ring before this read.
            let hdr = unsafe { (map as *const u8).add(header_abs) as *const Tpacket3Hdr };
            let snaplen = unsafe { (*hdr).tp_snaplen as usize };
            let mac = unsafe { (*hdr).tp_mac as usize };
            let (_header_abs, packet_off) = match checked_tpacket_packet_offsets(
                block_idx, block_size, ring_len, blk_len, off, mac, snaplen,
            ) {
                Some(value) if mac <= frame_size && snaplen <= frame_size => value,
                _ => {
                    warn!(worker, block_idx, packet_idx, off, mac, snaplen, blk_len, "rejecting invalid TPACKET_V3 packet range");
                    malformed = true;
                    break;
                }
            };
            // SAFETY: checked_tpacket_packet_offsets proves [packet_off,
            // packet_off + snaplen) lies in the current block and mapped ring.
            let packet = unsafe { std::slice::from_raw_parts((map as *const u8).add(packet_off), snaplen) };
            process_packet(packet, max_eps, trusted_overlay_cidrs, tx, budget, epoch, epoch_base);

            let next = unsafe { (*hdr).tp_next_offset as usize };
            match checked_next_tpacket_offset(off, next, packet_idx + 1 == num, blk_len, block_size) {
                Ok(Some(next_off)) => off = next_off,
                Ok(None) => {}
                Err(reason) => {
                    warn!(worker, block_idx, packet_idx, off, next, %reason, "rejecting invalid TPACKET_V3 descriptor chain");
                    malformed = true;
                    break;
                }
            }
        }
        std::sync::atomic::fence(Ordering::Release);
        unsafe { std::ptr::write_volatile(std::ptr::addr_of_mut!((*block_ptr).hdr.block_status), TP_STATUS_KERNEL); }
        block_idx = (block_idx + 1) % block_nr;
    }
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
        return if is_last { Ok(None) } else { Err("zero tp_next_offset before final packet") };
    }
    let aligned = next
        .checked_add(TPACKET_ALIGNMENT - 1)
        .ok_or("tp_next_offset alignment overflow")?
        & !(TPACKET_ALIGNMENT - 1);
    if aligned < std::mem::size_of::<Tpacket3Hdr>() {
        return Err("tp_next_offset does not advance past the current header");
    }
    let next_off = off.checked_add(aligned).ok_or("tp_next_offset addition overflow")?;
    checked_range_end(next_off, std::mem::size_of::<Tpacket3Hdr>(), blk_len)
        .ok_or("next packet header exceeds declared block")?;
    checked_range_end(next_off, std::mem::size_of::<Tpacket3Hdr>(), block_size)
        .ok_or("next packet header exceeds block size")?;
    Ok(Some(next_off))
}

#[cfg(target_os = "linux")]
fn set_sockopt<T>(fd: i32, level: i32, name: i32, value: &T) -> std::io::Result<()> {
    let rc = unsafe { libc::setsockopt(fd, level, name, (value as *const T).cast(), std::mem::size_of::<T>() as libc::socklen_t) };
    if rc < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(target_os = "linux")]
fn process_packet(packet: &[u8], max_eps: u64, trusted_overlay_cidrs: &[IpNetwork], tx: &Sender<ConnectionEvent>, budget: &Arc<AtomicU64>, epoch: &Arc<AtomicU64>, epoch_base: std::time::Instant) {
    let second = epoch_base.elapsed().as_secs();
    let seen = epoch.load(Ordering::Relaxed);
    if seen != second && epoch.compare_exchange(seen, second, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        budget.store(0, Ordering::Relaxed);
    }
    let (ip, proto, syn, _fragmented, overlay_identity) = match parse_l3(packet) { Some(v) => v, None => return };
    if trusted_overlay_cidrs.iter().any(|n| network_contains(n, ip)) && !overlay_identity {
        // A shared cloud/LB/node address is not a safe enforcement identity.
        // Do not feed it into local detection; tenant-aware telemetry must
        // supply the real client identity separately.
        return;
    }
    let admitted = budget
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            let next = n.checked_add(1)?;
            if next <= max_eps { Some(next) } else { None }
        })
        .is_ok();
    if !admitted { return; }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut fp = proto as u32;
    if syn { fp |= 0x4000_0000; }
    if overlay_identity { fp |= 0x8000_0000; }
    let ev = ConnectionEvent { ip, timestamp_ns: ts, bytes: packet.len() as u64, status_code: 0, proto_fingerprint: fp, l7: None };
    let _ = tx.try_send(ev);
}

#[cfg(target_os = "linux")]
fn parse_l3(frame: &[u8]) -> Option<(std::net::IpAddr, u8, bool, bool, bool)> {
    if frame.len() < 14 { return None; }
    let mut off = 14usize;
    let mut eth = u16::from_be_bytes([frame[12], frame[13]]);
    for _ in 0..4 {
        if eth != 0x8100 && eth != 0x88a8 { break; }
        if frame.len() < off + 4 { return None; }
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
fn parse_ipv4(frame: &[u8], off: usize) -> Option<(std::net::IpAddr, u8, bool, bool, bool)> {
    if frame.len() < off + 20 { return None; }
    let ihl = (frame[off] & 0x0f) as usize * 4;
    if ihl < 20 || frame.len() < off + ihl { return None; }
    let flags_frag = u16::from_be_bytes([frame[off + 6], frame[off + 7]]);
    let fragmented = (flags_frag & 0x1fff) != 0 || (flags_frag & 0x2000) != 0;
    let proto = frame[off + 9];
    let ip = std::net::Ipv4Addr::new(frame[off+12], frame[off+13], frame[off+14], frame[off+15]);
    let l4 = off + ihl;
    if fragmented { return Some((std::net::IpAddr::V4(ip), proto, false, true, false)); }

    // Native telemetry follows the same common overlay subset as XDP: IP-in-IP,
    // GRE without optional fields, VXLAN and Geneve with bounded option length.
    if proto == 4 && frame.len() >= l4 + 20 {
        let (ip, next, syn, frag, _) = parse_ipv4(frame, l4)?;
        return Some((ip, next, syn, frag, true));
    }
    if proto == 47 && frame.len() >= l4 + 4 {
        let flags = u16::from_be_bytes([frame[l4], frame[l4+1]]);
        let inner_proto = u16::from_be_bytes([frame[l4+2], frame[l4+3]]);
        if (flags & 0x4F00) == 0 && (inner_proto == 0x0800 || inner_proto == 0x86dd) {
            let mut inner = l4 + 4;
            if (flags & 0x8000) != 0 { inner += 4; }
            if (flags & 0x2000) != 0 { inner += 4; }
            if (flags & 0x1000) != 0 { inner += 4; }
            if inner_proto == 0x0800 {
                let (ip, next, syn, frag, _) = parse_ipv4(frame, inner)?;
                return Some((ip, next, syn, frag, true));
            }
            let (ip, next, syn, frag, _) = parse_ipv6(frame, inner)?;
            return Some((ip, next, syn, frag, true));
        }
    }
    if proto == 17 && frame.len() >= l4 + 8 {
        let sport = u16::from_be_bytes([frame[l4], frame[l4+1]]);
        let dport = u16::from_be_bytes([frame[l4+2], frame[l4+3]]);
        if sport == 4789 || dport == 4789 {
            if frame.len() < l4 + 16 || frame[l4 + 8] & 0x08 == 0 { return None; }
            let inner_eth = l4 + 16;
            if frame.len() >= inner_eth + 14 {
                let inner_type = u16::from_be_bytes([frame[inner_eth+12], frame[inner_eth+13]]);
                if inner_type == 0x0800 {
                    let (ip, next, syn, frag, _) = parse_ipv4(frame, inner_eth + 14)?;
                    return Some((ip, next, syn, frag, true));
                }
                if inner_type == 0x86dd {
                    let (ip, next, syn, frag, _) = parse_ipv6(frame, inner_eth + 14)?;
                    return Some((ip, next, syn, frag, true));
                }
            }
        } else if sport == 6081 || dport == 6081 {
            if frame.len() < l4 + 8 { return None; }
            if frame[l4] >> 6 != 0 { return None; }
            let opt_len = (frame[l4] & 0x3f) as usize * 4;
            let inner_eth = l4 + 8 + opt_len;
            if frame.len() >= inner_eth + 14 {
                let inner_type = u16::from_be_bytes([frame[inner_eth+12], frame[inner_eth+13]]);
                if inner_type == 0x0800 {
                    let (ip, next, syn, frag, _) = parse_ipv4(frame, inner_eth + 14)?;
                    return Some((ip, next, syn, frag, true));
                }
                if inner_type == 0x86dd {
                    let (ip, next, syn, frag, _) = parse_ipv6(frame, inner_eth + 14)?;
                    return Some((ip, next, syn, frag, true));
                }
            }
        }
    }
    let syn = proto == 6 && frame.len() >= l4 + 14 && frame[l4+13] & 0x02 != 0 && frame[l4+13] & 0x10 == 0;
    Some((std::net::IpAddr::V4(ip), proto, syn, false, false))
}

#[cfg(target_os = "linux")]
fn parse_ipv6(frame: &[u8], off: usize) -> Option<(std::net::IpAddr, u8, bool, bool, bool)> {
    if frame.len() < off + 40 { return None; }
    let mut b=[0u8;16]; b.copy_from_slice(&frame[off+8..off+24]);
    let mut next = frame[off+6];
    let mut cursor = off + 40;
    let mut fragmented = false;
    for _ in 0..8 {
        match next {
            0 | 43 | 60 => {
                if frame.len() < cursor + 2 { return None; }
                let len = (frame[cursor + 1] as usize + 1) * 8;
                next = frame[cursor]; cursor = cursor.checked_add(len)?;
                if frame.len() < cursor { return None; }
            }
            44 => { fragmented = true; if frame.len() < cursor + 8 { return None; } next = frame[cursor]; cursor += 8; }
            51 => { if frame.len() < cursor + 2 { return None; } let len=(frame[cursor+1] as usize+2)*4; next=frame[cursor]; cursor += len; if frame.len()<cursor{return None;} }
            50 => { return Some((std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)), next, false, true, false)); }
            _ => break,
        }
    }
    if !fragmented && next == 41 && frame.len() >= cursor + 40 {
        let (ip, proto, syn, frag, _) = parse_ipv6(frame, cursor)?;
        return Some((ip, proto, syn, frag, true));
    }
    if !fragmented && next == 47 && frame.len() >= cursor + 4 {
        let flags = u16::from_be_bytes([frame[cursor], frame[cursor + 1]]);
        let inner_proto = u16::from_be_bytes([frame[cursor + 2], frame[cursor + 3]]);
        if (flags & 0x4F00) == 0 && inner_proto == 0x86dd && frame.len() >= cursor + 44 {
            let mut inner = cursor + 4;
            if (flags & 0x8000) != 0 { inner += 4; }
            if (flags & 0x2000) != 0 { inner += 4; }
            if (flags & 0x1000) != 0 { inner += 4; }
            let (ip, proto, syn, frag, _) = parse_ipv6(frame, inner)?;
            return Some((ip, proto, syn, frag, true));
        }
    }
    if !fragmented && next == 17 && frame.len() >= cursor + 8 {
        let sport = u16::from_be_bytes([frame[cursor], frame[cursor + 1]]);
        let dport = u16::from_be_bytes([frame[cursor + 2], frame[cursor + 3]]);
        if sport == 4789 || dport == 4789 {
            if frame.len() < cursor + 16 || frame[cursor + 8] & 0x08 == 0 { return None; }
            let inner_eth = cursor + 16;
            if frame.len() >= inner_eth + 14 {
                let et = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if et == 0x86dd {
                    let (ip, proto, syn, frag, _) = parse_ipv6(frame, inner_eth + 14)?;
                    return Some((ip, proto, syn, frag, true));
                } else if et == 0x0800 {
                    let (ip, proto, syn, frag, _) = parse_ipv4(frame, inner_eth + 14)?;
                    return Some((ip, proto, syn, frag, true));
                }
            }
        } else if sport == 6081 || dport == 6081 {
            if frame.len() < cursor + 8 || frame[cursor] >> 6 != 0 { return None; }
            let opt_len = (frame[cursor] & 0x3f) as usize * 4;
            let inner_eth = cursor + 8 + opt_len;
            if frame.len() >= inner_eth + 14 {
                let et = u16::from_be_bytes([frame[inner_eth + 12], frame[inner_eth + 13]]);
                if et == 0x86dd {
                    let (ip, proto, syn, frag, _) = parse_ipv6(frame, inner_eth + 14)?;
                    return Some((ip, proto, syn, frag, true));
                } else if et == 0x0800 {
                    let (ip, proto, syn, frag, _) = parse_ipv4(frame, inner_eth + 14)?;
                    return Some((ip, proto, syn, frag, true));
                }
            }
        }
    }
    let syn = !fragmented && next == 6 && frame.len() >= cursor + 14 && frame[cursor+13] & 0x02 != 0 && frame[cursor+13] & 0x10 == 0;
    Some((std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)), next, syn, fragmented, false))
}

fn network_contains(network: &IpNetwork, ip: std::net::IpAddr) -> bool {
    match (network.addr, ip) {
        (std::net::IpAddr::V4(net), std::net::IpAddr::V4(host)) => {
            let n = u32::from(net);
            let h = u32::from(host);
            let bits = network.prefix_len;
            if bits == 0 { true } else { (n & (!0u32 << (32 - bits))) == (h & (!0u32 << (32 - bits))) }
        }
        (std::net::IpAddr::V6(net), std::net::IpAddr::V6(host)) => {
            let n = u128::from(net);
            let h = u128::from(host);
            let bits = network.prefix_len;
            if bits == 0 { true } else { (n & (!0u128 << (128 - bits))) == (h & (!0u128 << (128 - bits))) }
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
        assert!(checked_tpacket_header_offset(0, 128, 128, 128, desc).is_some());
    }

    #[test]
    fn checked_range_rejects_offset_overflow() {
        assert!(checked_range_end(usize::MAX - 1, 8, usize::MAX).is_none());
        assert!(checked_next_tpacket_offset(usize::MAX - 8, 32, false, usize::MAX, usize::MAX).is_err());
    }

    #[test]
    fn rejects_packet_range_past_block_and_mapped_ring() {
        // Header [48, 96) is valid, but packet [128, 129) crosses the block.
        assert!(checked_tpacket_packet_offsets(0, 128, 128, 128, 48, 80, 1).is_none());
        // On the final block, packet [255, 257) crosses both block and mapping.
        assert!(checked_tpacket_packet_offsets(1, 128, 256, 128, 48, 79, 2).is_none());
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
        assert_eq!(checked_next_tpacket_offset(48, 48, false, 128, 128), Ok(Some(96)));
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
        let (src, proto, syn, fragmented, overlay) = parse_l3(&frame).unwrap();
        assert_eq!(src, "198.51.100.7".parse::<std::net::IpAddr>().unwrap());
        assert_eq!(proto, 6);
        assert!(syn);
        assert!(!fragmented);
        assert!(!overlay);
    }

    #[test]
    fn rejects_ipv4_fragment_as_transport_identity() {
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        ip[9] = 17;
        ip[12..16].copy_from_slice(&[198, 51, 100, 8]);
        let frame = eth(0x0800, &ip);
        let (_, _, syn, fragmented, _) = parse_l3(&frame).unwrap();
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
        let (src, proto, syn, fragmented, overlay) = parse_l3(&frame).unwrap();
        assert_eq!(src, std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
        assert_eq!(proto, 17);
        assert!(!syn);
        assert!(!fragmented);
        assert!(!overlay);
    }
}
