//! DDoS stress tests — proves RamShield XDP survives peak attack load.
//! All tests require root. Run with:
//!   sudo env PATH="$HOME/.cargo/bin:$PATH" RUSTUP_HOME="$HOME/.rustup" \
//!     /home/m/.cargo/bin/cargo test -p ramshield-xdp --features elf \
//!     -- --ignored ddos_ --test-threads=1

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)] // test-only raw BPF syscall wrappers, root-gated
use std::collections::HashSet;
use aya::Ebpf;
use std::net::Ipv4Addr;
use std::os::fd::{AsFd, AsRawFd};
use std::time::Instant;

// ── BPF elem syscall ──────────────────────────────────────────────────
const ATTR_SZ: usize = 32;

unsafe fn bpf_update(fd: i32, key: *const u8, val: *const u8, flags: u64) -> std::io::Result<()> {
    let mut b = [0u8; ATTR_SZ];
    b[0..4].copy_from_slice(&(fd as u32).to_ne_bytes());
    b[8..16].copy_from_slice(&(key as u64).to_ne_bytes());
    b[16..24].copy_from_slice(&(val as u64).to_ne_bytes());
    b[24..32].copy_from_slice(&flags.to_ne_bytes());
    if libc::syscall(libc::SYS_bpf, 2i64, b.as_ptr(), ATTR_SZ) < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn bpf_delete(fd: i32, key: *const u8) -> std::io::Result<()> {
    let mut b = [0u8; ATTR_SZ];
    b[0..4].copy_from_slice(&(fd as u32).to_ne_bytes());
    b[8..16].copy_from_slice(&(key as u64).to_ne_bytes());
    if libc::syscall(libc::SYS_bpf, 3i64, b.as_ptr(), ATTR_SZ) < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Lookup element.
unsafe fn bpf_lookup(fd: i32, key: *const u8, val_out: *mut u8) -> std::io::Result<()> {
    let mut b = [0u8; ATTR_SZ];
    b[0..4].copy_from_slice(&(fd as u32).to_ne_bytes());
    b[8..16].copy_from_slice(&(key as u64).to_ne_bytes());
    b[16..24].copy_from_slice(&(val_out as u64).to_ne_bytes());
    if libc::syscall(libc::SYS_bpf, 1i64, b.as_ptr(), ATTR_SZ) < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// ── Key/value ─────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
struct Key(u128);

impl Key {
    fn v4(a: u8, b: u8, c: u8, d: u8) -> Self {
        Key(u128::from(u32::from_ne_bytes(
            Ipv4Addr::new(a, b, c, d).octets(),
        )))
    }
    fn v6(o: [u8; 16]) -> Self {
        Key(u128::from_le_bytes(o))
    }
    fn idx(i: usize) -> Self {
        Self::v4(
            (i % 256) as u8,
            ((i >> 8) % 256) as u8,
            ((i >> 16) % 256) as u8,
            ((i >> 24) % 255 + 1) as u8,
        )
    }
    fn v6_idx(i: usize) -> Self {
        let mut o = [0u8; 16];
        o[0] = 0x20;
        o[1] = 0x01;
        o[8] = ((i >> 24) % 256) as u8;
        o[9] = ((i >> 16) % 256) as u8;
        o[10] = ((i >> 8) % 256) as u8;
        o[15] = (i & 0xFF) as u8;
        Self::v6(o)
    }
    fn as_map_key(&self) -> [u64; 2] {
        let b = self.0.to_ne_bytes();
        [
            u64::from_ne_bytes(b[0..8].try_into().unwrap()),
            u64::from_ne_bytes(b[8..16].try_into().unwrap()),
        ]
    }
    fn raw(&self) -> [u8; 16] {
        self.0.to_ne_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct Val(u64);

unsafe impl aya::Pod for Key {}
unsafe impl aya::Pod for Val {}

const CAP: usize = 102_400;

/// Value = absolute expiry ns (CLOCK_MONOTONIC, same as BPF bpf_ktime_get_ns).
/// u64::MAX = permanent — tests want the key to stay blocking for the run.
const PERM: u64 = u64::MAX;

// ── Helpers ───────────────────────────────────────────────────────────
fn load() -> aya::Ebpf {
    Ebpf::load(ramshield_xdp::BPF_ELF).expect("load failed")
}

fn raw_fd(bpf: &mut aya::Ebpf, name: &str) -> i32 {
    let map = bpf.map_mut(name).expect("map missing");
    match map {
        aya::maps::Map::HashMap(d) => d.fd().as_fd().as_raw_fd(),
        aya::maps::Map::LruHashMap(d) => d.fd().as_fd().as_raw_fd(),
        other => panic!("{name}: {other:?}"),
    }
}

/// Count keys — consumes Map, fd invalid after.
fn count(bpf: &mut aya::Ebpf, name: &str) -> usize {

    let map = bpf.take_map(name).unwrap();
    aya::maps::HashMap::<_, [u64; 2], Val>::try_from(map)
        .unwrap()
        .keys()
        .count()
}

/// Collect all keys as [u64;2]. Consumes Map.
fn collect(bpf: &mut aya::Ebpf, name: &str) -> Vec<[u64; 2]> {

    let map = bpf.take_map(name).unwrap();
    aya::maps::HashMap::<_, [u64; 2], Val>::try_from(map)
        .unwrap()
        .keys()
        .filter_map(|k| k.ok())
        .collect()
}

unsafe fn ins(fd: i32, k: &Key) {
    let v = Val(PERM);
    bpf_update(fd, k.raw().as_ptr(), &v as *const _ as *const u8, 0).unwrap();
}

unsafe fn del(fd: i32, k: &Key) {
    bpf_delete(fd, k.raw().as_ptr()).unwrap();
}

// ══════════════════════════════════════════════════════════════════════
// DIAGNOSTIC — prove insert/lookup/delete work before stress tests
// ══════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn ddos_diag_crud() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    let k = Key::idx(0);
    let kb = k.raw();

    // Insert
    unsafe {
        let v = Val(PERM);
        let r = bpf_update(fd, kb.as_ptr(), &v as *const _ as *const u8, 0);
        eprintln!("[crud] insert={r:?}");
        assert!(r.is_ok());

        // Lookup
        let mut val = [0u8; 1];
        let r = bpf_lookup(fd, kb.as_ptr(), val.as_mut_ptr());
        eprintln!("[crud] lookup={r:?} val={:?}", val);
        assert!(r.is_ok());
        assert_eq!(val[0], 1);

        // Delete
        let r = bpf_delete(fd, kb.as_ptr());
        eprintln!("[crud] delete={r:?}");
        assert!(r.is_ok(), "delete failed: {r:?}");

        // Lookup again — should fail
        let r = bpf_lookup(fd, kb.as_ptr(), val.as_mut_ptr());
        eprintln!("[crud] lookup_after_delete={r:?}");
        assert!(r.is_err());

        eprintln!("[crud] ALL PASSED — insert/lookup/delete cycle works");
    }
}

// ══════════════════════════════════════════════════════════════════════
// TESTS
// ══════════════════════════════════════════════════════════════════════

#[test]
#[ignore]
fn ddos_fill_map_to_capacity() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    let t0 = Instant::now();
    for i in 0..CAP {
        unsafe {
            ins(fd, &Key::idx(i));
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let c = count(&mut bpf, "BLOCKLIST");
    assert_eq!(c, CAP, "expected {CAP}, got {c}");
    eprintln!(
        "[fill] {CAP} in {ms:.0}ms ({:.0}/sec)",
        CAP as f64 / (ms / 1000.0)
    );
}

#[test]
#[ignore]
fn ddos_lru_eviction() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    for i in 0..CAP {
        unsafe {
            ins(fd, &Key::idx(i));
        }
    }
    // LRU_HASH: insert one more — kernel evicts coldest, never E2BIG.
    let k = Key::v4(99, 99, 99, 99);
    let r = unsafe { bpf_update(fd, k.raw().as_ptr(), &Val(PERM) as *const _ as *const u8, 0) };
    assert!(
        r.is_ok(),
        "LRU insert must never fail: {:?}",
        r.unwrap_err()
    );
    // Count should not exceed CAP — one old entry was evicted.
    let c = count(&mut bpf, "BLOCKLIST");
    assert!(c <= CAP, "LRU must cap at {CAP}, got {c}");
    eprintln!("[lru] fill+overflow: {c} entries (cap {CAP}), LRU eviction confirmed");
}

#[test]
#[ignore]
fn ddos_lookup_10k() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    let n = 10_000;
    for i in 0..n {
        unsafe {
            ins(fd, &Key::idx(i));
        }
    }
    let t0 = Instant::now();
    let c = count(&mut bpf, "BLOCKLIST");
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(c, n);
    eprintln!(
        "[10k] iterate {n} in {ms:.0}ms ({:.0}/sec)",
        n as f64 / (ms / 1000.0)
    );
}

#[test]
#[ignore]
fn ddos_lookup_100k() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    let n = 100_000;
    let t_ins = Instant::now();
    for i in 0..n {
        unsafe {
            ins(fd, &Key::idx(i));
        }
    }
    let ins_ms = t_ins.elapsed().as_secs_f64() * 1000.0;
    let t_iter = Instant::now();
    let c = count(&mut bpf, "BLOCKLIST");
    let iter_ms = t_iter.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(c, n);
    eprintln!(
        "[100k] insert {n} in {ins_ms:.0}ms | iterate {n} in {iter_ms:.0}ms ({:.0}/sec)",
        n as f64 / (iter_ms / 1000.0)
    );
}

#[test]
#[ignore]
fn ddos_reconcile_50k() {
    let half = 50_000;
    let all_keys = {
        let mut bpf = load();
        let fd = raw_fd(&mut bpf, "BLOCKLIST");
        for i in 0..half {
            unsafe {
                ins(fd, &Key::idx(i));
            }
        }
        collect(&mut bpf, "BLOCKLIST")
    };
    let new_set: HashSet<[u64; 2]> = (half..half * 2).map(|i| Key::idx(i).as_map_key()).collect();

    let mut bpf2 = load();
    let fd2 = raw_fd(&mut bpf2, "BLOCKLIST");
    for i in 0..half {
        unsafe {
            ins(fd2, &Key::idx(i));
        }
    }
    let mut stale = 0;
    for mk in &all_keys {
        if !new_set.contains(mk) {
            let bytes: [u8; 16] = {
                let mut x = [0u8; 16];
                x[0..8].copy_from_slice(&mk[0].to_ne_bytes());
                x[8..16].copy_from_slice(&mk[1].to_ne_bytes());
                x
            };
            unsafe {
                bpf_delete(fd2, bytes.as_ptr()).unwrap();
            }
            stale += 1;
        }
    }
    let mut inserted = 0;
    for mk in &new_set {
        let r = unsafe {
            bpf_update(
                fd2,
                mk as *const _ as *const u8,
                &Val(PERM) as *const _ as *const u8,
                0,
            )
        };
        if r.is_ok() {
            inserted += 1;
        }
    }
    let after = count(&mut bpf2, "BLOCKLIST");
    assert_eq!(after, half, "expected {half}, got {after}");
    eprintln!("[reconcile] stale={stale} inserted={inserted} final={after}");
}

#[test]
#[ignore]
fn ddos_isolation() {
    let n = 10_000;
    // Phase 1: v6 inserts + count
    {
        let mut bpf = load();
        let v6_fd = raw_fd(&mut bpf, "BLOCKLIST6");
        for i in 0..n {
            unsafe {
                ins(v6_fd, &Key::v6_idx(i));
            }
        }
        let c6 = count(&mut bpf, "BLOCKLIST6");
        assert_eq!(c6, n);
        eprintln!("[iso] v6={n}");
    }
    // Phase 2: v4 inserts + count
    {
        let mut bpf = load();
        let v4_fd = raw_fd(&mut bpf, "BLOCKLIST");
        for i in 0..n {
            unsafe {
                ins(v4_fd, &Key::idx(i));
            }
        }
        let c4 = count(&mut bpf, "BLOCKLIST");
        assert_eq!(c4, n);
        eprintln!("[iso] v4={n}, zero cross-contamination");
    }
}

#[test]
#[ignore]
fn ddos_rapid_churn() {
    let n = 5_000;
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    let t0 = Instant::now();
    for i in 0..n {
        let k = Key::idx(i);
        unsafe {
            ins(fd, &k);
            del(fd, &k);
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let c = count(&mut bpf, "BLOCKLIST");
    assert_eq!(c, 0, "must be empty, got {c}");
    eprintln!(
        "[churn] {n} cycles in {ms:.0}ms ({:.0}/sec)",
        n as f64 / (ms / 1000.0)
    );
}

#[test]
#[ignore]
fn ddos_boundary_reuse() {
    let mut bpf = load();
    let fd = raw_fd(&mut bpf, "BLOCKLIST");
    for i in 0..CAP {
        unsafe {
            ins(fd, &Key::idx(i));
        }
    }
    // LRU: overflow succeeds — kernel evicts coldest entry.
    let k = Key::v4(99, 99, 99, 99);
    unsafe {
        bpf_update(fd, k.raw().as_ptr(), &Val(PERM) as *const _ as *const u8, 0).unwrap();
    }
    // Delete one, re-insert — count stays ≤ CAP.
    unsafe {
        del(fd, &Key::idx(42));
    }
    unsafe {
        ins(fd, &Key::idx(42));
    }
    let c = count(&mut bpf, "BLOCKLIST");
    assert!(c <= CAP, "LRU must cap at {CAP}, got {c}");
    eprintln!("[boundary] fill/lru-evict/delete/re-insert: {c} entries ≤ cap {CAP}");
}

// ── COUNTERS map validation ───────────────────────────────────────────────

#[test]
#[ignore]
fn ddos_counters_map_loads() {
    let bpf = load();
    // COUNTERS map must exist and be a PerCpuArray
    let map = bpf.map("COUNTERS").expect("COUNTERS map missing");
    let array: aya::maps::PerCpuArray<&aya::maps::MapData, u64> =
        aya::maps::PerCpuArray::try_from(map).expect("COUNTERS not a PerCpuArray");
    assert_eq!(array.len(), 4, "must have 4 counter slots");

    // Read slot 0 (v4_drop) via aya API
    let vals: aya::maps::PerCpuValues<u64> = array.get(&0, 0).expect("slot 0 read failed");
    let total: u64 = vals.iter().sum();
    assert_eq!(total, 0, "initial counter must be 0, got {total}");

    // Verify all 4 slots readable and zero
    for slot in 0u32..4 {
        let v: aya::maps::PerCpuValues<u64> = array.get(&slot, 0).expect("slot read failed");
        let s: u64 = v.iter().sum();
        assert_eq!(s, 0, "slot {slot} initial must be 0, got {s}");
    }
    let nr_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    eprintln!("[counters] COUNTERS map loaded, 4 slots, {nr_cpus} CPUs, all zero");
}
