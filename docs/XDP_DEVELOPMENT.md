# Express Data Path (XDP): the technology, its implementation, and RamShield's take

## 1. What XDP is

XDP (Express Data Path) is a Linux kernel feature that runs an eBPF program at the earliest possible point in the packet receive path — right after the NIC hands a frame to the driver, before the kernel allocates an `sk_buff` (socket buffer). Because the decision runs before the expensive skb allocation, XDP is the fastest programmable in-kernel packet path available on Linux.

Core model (kernel docs, `BPF_PROG_TYPE_XDP`):

- Hook: ingress on a network device; one program per interface, called for every incoming packet.
- Context: `struct xdp_md` with `data`/`data_end` pointers bracketing the packet, plus `ingress_ifindex`, `rx_queue_index`, `egress_ifindex`, `data_meta`.
- Return actions: `XDP_DROP` (discard before stack), `XDP_PASS` (hand to kernel network stack), `XDP_TX` (send back out same port), `XDP_REDIRECT` (another port/CPU/userspace), `XDP_ABORTED` (error tracepoint).
- Boundaries: every read must prove `data + offset + len <= data_end` or the verifier rejects the program; off by one and nothing loads.

Attachment modes:

| Mode | What it means | Cost |
|------|---------------|------|
| driver / native | runs in NIC driver, pre-skb | fastest (~1M pps/core), needs driver support |
| generic / skb | runs after skb allocation | works everywhere, skb cost largely paid |
| offload | runs on NIC hardware | fastest for hardware that supports it |

Attachment lifecycle: legacy netlink attach holds a reference so the program survives loader exit; modern BPF links (`BPF_LINK_TYPE_XDP`) tie lifetime to the link fd — close or unpin and the program detaches. `XDP_FLAGS_SKB_MODE` is mutually exclusive with driver/hw mode; hardware offload requires GRO/LRO disabled (`ethtool -K` gro/lro off) or attach fails with `-EINVAL`.

Frames larger than ~1500–4096 bytes may not fit the linear `data..data_end` window on older drivers; `BPF_F_XDP_HAS_FRAGS` plus `bpf_xdp_load_bytes`/`bpf_xdp_store_bytes` handle multi-page packets where supported.

## 2. How XDP maps work with userspace

XDP programs are immutable after load, but their data is not: they read BPF maps that userspace can update at runtime. A blocklist is a classic example — userspace inserts blocked source IPs, the dataplane looks each packet up and drops matches without crossing to userspace.

Map types RamShield uses (in `crates/ramshield-xdp/ramshield-xdp-bpf/src/main.rs`):

- `LruHashMap<[u64; 2], u64>` for `BLOCKLIST` / `BLOCKLIST6` — key is a 16-byte `__u64[2]` value; IPv4 occupies bytes `[0..4]` in wire order, rest zeroed; value is an absolute expiry in `CLOCK_MONOTONIC` ns (`u64::MAX` = permanent). LRU means the kernel auto-evicts the coldest entry under botnet floods — no `E2BIG` map-full failures. Cap set via `BLOCKLIST_CAP` env at build (default 102_400).
- `LpmTrie<[u64; 2], u8>` for `BLOCKCIDR` / `BLOCKCIDR6` — prefix-length keyed longest-prefix match; one /24 entry replaces 256 flat /32 blocklist entries.
- `RingBuf` (`EVENTS`, 64 KiB) for kernel→userspace drop notifications: `{ ip: [u8;16], ts_ns: u64, action: u8, slot: u8 }`.
- `PerCpuArray<u64>` (`COUNTERS`, 4 slots) for per-CPU drop/pass/parse-fail counters — read-modify-write per CPU slot with no atomics or locks (XDP is non-preemptible per CPU).

The timing contract: userspace `apply_block` computes `now_ns + ttl*1e9` via `clock_gettime(CLOCK_MONOTONIC)` — the same clock BPF reads with `bpf_ktime_get_ns`. The dataplane drops only while `now < expiry`. A resident key keeps whatever expiry `apply_block` wrote; reconcile does not rewrite it to `PERMANENT`, so a missed keep-alive lets the block lapse at its TTL instead of living forever.

Key byte layout is the one place where silent misbehavior hides: the C/aya dataplane copies raw wire octets into `key[0]`. Userspace must serialize IPv4 as wire-order octets at `mem[0..4]` (zero pad through `mem[15]`), IPv6 as all 16 wire octets. `u32::from_ne_bytes` on little-endian is correct; `u32::from` byte-reverses and makes every lookup miss. RamShield pins this with wire-contract unit tests in both `ramshield-xdp` and `ramshield-enforcement`, plus a `parse_drop_event` that reconstructs the IP from slot 0 (v4) vs slot 1 (v6).

## 3. Userspace side: the enforcement layer

`ramshield-enforcement::xdp::AyaXdpApplier` (aya 0.14) loads the compiled BPF ELF (`ramshield_xdp::BPF_ELF`, produced by `bpf-linker` + `cargo build --target bpfel-unknown-none`), attaches it with `XdpMode::Driver` for `"native"`/`"drv"` and `Skb` for `"skb"` — never silently downgrades explicit native to SKB.

Semantics:

- **Fail-open at the kernel**: map update failure returns `Err`; callers keep in-band state and traffic keeps flowing.
- **Reconcile**: drain the kernel map once via raw `bpf(2)` `BPF_MAP_GET_NEXT_KEY` (aya's borrow rules forbid iterating while mutating), stage stale keys for deferred deletion, insert only expected keys that are absent. Never rewrites an already-correct key — that avoids contending the same bucket lock the data path takes per packet. Stale keys are staged before deletion so the enumeration cursor advances past them; otherwise `get_next_key` returns the same resident key forever and the control plane wedges.
- **CIDR reconcile**: separate LPM-trie sweep per family, keyed by `(prefix_len, data)` hash set — O(resident + expected) instead of O(resident × expected).
- **D2 isolation**: IPv4 keys go to `BLOCKLIST`, IPv6 to `BLOCKLIST6`, reconciled independently. A v6 key must never enter the v4 sweep or vice versa.

Projection path on a block command: WAL append (durable, `fsync`) → store mutation → `apply_block` writes the map → mesh broadcast (local/operator origin only, to avoid gossip amplification). XDP stays outside the checkpoint barrier so a dataplane stall can't block WAL durability.

Observability: `drain_drop_events` reads the ringbuf each 250 ms tick, `counters()` sums the per-CPU array, drops are attributed only to userspace-blocked IPs (observability only — never fed back into detection, to avoid self-confirming false positives).

## 4. Design takeaways (RamShield's view)

1. **Drop early, observe late.** `XDP_DROP` is invisible to `tcpdump` (packet never reaches the stack), so visibility must come from maps (ringbuf events, counters) — not from post-stack tools. RamShield instruments every drop path and surfaces `v4_drop/v6_drop/pass/parse_fail` as Prometheus metrics.

2. **Fail-open is a product decision, not an accident.** Map update failure → continue without blocking. The alternative (fail-closed, drop everything when the map is unreachable) is a denial-of-service vector against your own dataplane.

3. **Byte layout is a security boundary.** The userspace↔kernel key contract is verified by unit tests, not by hope. Endianness bugs here don't crash; they silently let blocked IPs through.

4. **LRU > flat map for flood safety.** A fixed-size flat hashmap fills and returns `E2BIG` under a botnet swarm; LRU auto-evicts cold entries so enforcement stays available at the cost of occasionally re-blocking a recently-seen attacker.

5. **Reconcile heals drift without pausing the data path.** Stale-key deletion and missing-key insertion run out-of-band; resident correct keys are left alone. The 10 s periodic reconcile plus per-command projection means driver reloads or external map wipes self-heal within one cycle.

6. **SKB mode is a compatibility escape hatch, not a target.** `allow_inband_fallback = false` in the smoke config forces driver mode; if the runner kernel lacks XDP support the gate fails loudly rather than silently degrading to generic-mode performance.

## 5. What the smoke gate proves

`scripts/xdp_netns_smoke.sh` runs the real kernel path: veth pair + network namespace, XDP (skb) on the host veth, telemetry over IPC blocks `10.200.0.2`, then asserts the post-block `connect` times out (SYN silently dropped) instead of failing fast with RST. Metrics assert `ramshield_blocks_detection > 0`; the kernel-side drop timing is the proof the block reconciled into the BPF map. Diagnostics on failure dump `ip -s link`, `/proc/net/packet`, an independent AF_PACKET tap, tcpdump cross-check, and a `lo` control tap to separate veth/xdpgeneric starvation from machine-wide tap breakage (issue #90 instrumentation).

---

Sources: docs.ebpf.io `BPF_PROG_TYPE_XDP`; kernel.org `Documentation/networking/filter.html`; RamShield sources `crates/ramshield-xdp`, `crates/ramshield-enforcement/src/xdp.rs`, `scripts/xdp_netns_smoke.sh`, `.github/workflows/xdp_netns_gate.yml`.

// ponytail: this article summarizes existing docs + code; no new code written. Add a formal threat model for XDP bypass (e.g. VLAN tag spoofing beyond 4 tags, LRO interaction) when the gate runs green for a month.