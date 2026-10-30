# The Packet That Never Reached the Kernel

### How XDP turns a DDoS response from "call the firewall" into "drop it before the operating system wakes up" — and how RamShield makes that idea survive contact with production.

There is a specific sound a production box makes when it is being flooded.

Not the fans. The fans are already loud. It's the absence of everything else: no application logs, no health checks, no SSH banner, just a wall of kernel softirq time and a `ss` output that refreshes into a different kind of wrong every second. You are watching a machine drown in packets it will never usefully process.

The instinct is to reach for iptables, or a WAF, or a load balancer's "block this IP" button. They all work. Eventually. But there is a physics problem baked into the Linux networking stack: by the time most filtering tools see a packet, the kernel has already allocated a socket buffer for it. An `sk_buff`. A few hundred bytes of metadata, a trip through the receive path, a hash lookup, a qdisc, a TCP state machine — all of that happens before your rule says "actually, no."

XDP asks a different question.

What if the "no" happened first?

---

## The hook before the hook

XDP — Express Data Path — is an eBPF program that runs at the earliest programmable point in the receive path. On a NIC with driver support, the frame has been DMA'd into memory, the driver has a descriptor in hand, and *before* the kernel builds an `sk_buff` for it, your program gets a pointer to the raw bytes.

That's the entire trick. It's also why XDP is fast. You are no longer paying for the object you're about to throw away.

The kernel gives your program a tiny context, `struct xdp_md`:

```c
struct xdp_md {
    __u32 data;        // start of packet
    __u32 data_end;    // end of packet
    __u32 data_meta;   // room for metadata
    __u32 ingress_ifindex;
    __u32 rx_queue_index;
    __u32 egress_ifindex;
};
```

Two pointers and a length. Everything you can do starts and ends there.

You return one of five actions:

- `XDP_DROP` — gone. Not "queued for later review." Gone before the stack knows it existed.
- `XDP_PASS` — hand it to the normal network stack, untouched.
- `XDP_TX` — bounce it back out the same interface.
- `XDP_REDIRECT` — send it to another port, another CPU, or userspace via AF_XDP.
- `XDP_ABORTED` — something went wrong; used for debugging, not production.

The verifier is the price of admission. Every pointer dereference must be provably inside `data..data_end`, and the proof has to hold on every path through the program. No dynamic dispatch, no libraries, no `printf`. Miss a bounds check and the program simply refuses to load. It's strict in a way that feels personal the first time it rejects you.

But the strictness is why XDP is trustworthy in a way userspace filtering never quite is. A program that loads is a program that cannot read past the packet.

---

## Three modes, three different physics problems

XDP attaches in one of three modes, and the mode is not a footnote. It changes the performance model completely.

**Native (driver) mode.** The program runs inside the NIC driver, before `sk_buff` allocation. This is the real XDP. Roughly a million packets per second per core is the number people throw around, and on modern hardware it's not marketing.

**Generic (SKB) mode.** The program runs after the kernel has already allocated the socket buffer. You get the programming model, but you've already paid the cost you were trying to avoid. It's a compatibility escape hatch, not a target architecture.

**Offload mode.** The program runs on the NIC itself. The fastest option, if your hardware and firmware cooperate. Also the one with the longest list of caveats.

There's an operational detail that bites everyone once: hardware offload requires GRO and LRO disabled. If you attach to a device with LRO on, you get `-EINVAL` and a stack trace that does not mention LRO anywhere. And on older drivers, a packet larger than ~1500–4096 bytes may not fit the linear window at all, which is why the kernel grew fragment-aware XDP programs and `bpf_xdp_load_bytes`.

The lesson is unglamorous: XDP is not "fast iptables." It is a different attachment point with its own constraints, and half of production XDP work is knowing which mode you're actually in.

---

## The map is the control plane

An XDP program is immutable once loaded. That sounds useless until you remember that programs can read eBPF maps, and maps are mutable from userspace at runtime.

This is the architecture that makes XDP a product rather than a demo:

**the data plane looks up every packet in a map; the control plane updates the map.**

In RamShield, the blocklist is an `LruHashMap` keyed by a 16-byte `[u64; 2]` value. IPv4 lives in the first four bytes, in wire order; the rest is zeroed. The value is not a boolean. It's an absolute expiry timestamp on the monotonic clock:

```rust
if let Some(v) = unsafe { BLOCKLIST.get(&key) } {
    let now = unsafe { bpf_ktime_get_ns() };
    if now < *v {
        return Ok(xdp_action::XDP_DROP);
    }
}
```

That one comparison replaces an entire class of control-plane complexity. No timers in the BPF program. No "unblock" race where the userspace process dies and the block lives forever. The map says "drop until T," and after T the packet is passed again — automatically, in the kernel, with no second syscall.

There are two more maps doing quiet, important work:

- An **LPM trie** for CIDR blocks. One `/24` entry replaces 256 flat `/32` entries, which matters when a scanner is walking your entire address space and you want to answer with a prefix, not a list.
- A **ring buffer** for drop events. XDP drops are invisible to `tcpdump` — the packet never reaches the packet socket — so if you want observability you have to build it. The ringbuf carries `{ IP, timestamp, action, slot }` records back to userspace every 250 ms.

And a **per-CPU counter array** for the health of the data plane itself: drops v4, drops v6, passes, parse failures. Per-CPU slots mean the data path never touches an atomic. The XDP program runs non-preemptibly on its CPU; a plain increment is enough.

---

## The bug that makes XDP silently stop working

Here's the part of the story that separates a demo from a security product.

The map key is a byte contract between two languages and two memory models. The eBPF program copies the raw source address bytes out of the IP header:

```rust
let src_bytes = unsafe { (*ip).src_addr };   // [u8; 4], wire order
let mut key = [0u64; 2];
unsafe {
    core::ptr::copy_nonoverlapping(
        src_bytes.as_ptr(),
        key.as_mut_ptr() as *mut u8,
        4,
    );
}
```

The userspace side has to serialize the same bytes. On little-endian x86, there are two ways that look almost identical and behave completely differently:

```rust
// Wrong: byte-reverses on LE. Every lookup misses.
let bad = u32::from(v4) as u64;

// Right: copies network-order octets verbatim.
bytes[0..4].copy_from_slice(&v4.octets());
```

Get it wrong and nothing crashes. The program loads, the map fills, the dashboard goes green, and every packet sails through the lookup as if the blocklist were empty. The failure is silent, which is the worst kind of failure a security control can have.

RamShield pins this contract with wire-format unit tests on both sides of the boundary — the same octets, the same padding, the same 16-byte key, asserted rather than assumed. It is the least glamorous code in the repo and probably the most important.

Security engineers like to say "never trust the network." The corollary is "never trust the serialization." The network will at least fail loudly. A key layout mismatch won't.

---

## The flood, and the map that doesn't fill

Once the data plane can drop, the next problem is scale. A flat hash map has a maximum size. A botnet does not.

The classic failure mode is embarrassingly simple: the blocklist hits its entry limit, the map update returns `E2BIG`, the enforcement layer logs an error, and — if it's fail-closed — your data plane starts dropping legitimate traffic. Or if it's fail-open, it quietly stops blocking.

RamShield uses an **LRU hash map**. When the map is full, the kernel evicts the coldest entry to make room. Under a botnet flood, that means the blocklist stays available at the cost of occasionally re-blocking a recently-seen attacker. That's a deliberate trade: availability of the control wins over completeness of the list.

There's a second-order effect that took time to get right: userspace must reconcile. The map is not the source of truth; the store and the WAL are. If the driver reloads, or the map is wiped externally, or a key is evicted, the kernel and the control plane drift apart.

So the enforcement loop reconciles every ten seconds, and after every block command. It enumerates the map once with raw `bpf(2)` calls — aya's borrow rules don't let you iterate and mutate through safe handles — computes a plan, deletes stale keys, and inserts only the expected keys that are absent.

That last word carries the design:

> **Insert only what's missing.**

Re-inserting an already-correct key costs one `bpf_map_update_elem` syscall per blocked IP per cycle, and that syscall takes the same bucket lock the XDP data path takes on every single packet. At a large blocklist, the "safety" sweep becomes a competitor to the thing it's protecting.

The boring version — leave correct keys alone — is faster and more correct. It also required fixing a subtle enumeration bug: if you stage a stale key for deletion but don't advance the cursor past it, `BPF_MAP_GET_NEXT_KEY` hands you the same resident key forever, and the control plane wedges. The fix is one line of cursor discipline, and the regression test exists because that one line is load-bearing.

---

## Proving it, not believing it

The hardest part of shipping any kernel-level security feature is that the demo is easy and the proof is hard.

You can attach an XDP program and watch it drop packets in five minutes. You can also attach one that *appears* to drop packets because the traffic you're testing with never arrived in the first place. The failure modes look identical from a dashboard.

RamShield's smoke gate runs the real thing in CI: a network namespace, a veth pair, the actual binary, XDP on the host veth, and a block driven over the real IPC wire. Then it asserts something almost childish:

> after the block, the same TCP connect must **not** fail fast.

Before the block, connecting to a closed port returns `ECONNREFUSED` in milliseconds — the host sends a RST. After the block, XDP discards the SYN, so the connect hangs until the timeout. The timing *is* the proof. A 3 ms refusal means the packet reached the stack and XDP never dropped it. A 3-second timeout means the kernel threw it away before the operating system noticed.

That single assertion catches an entire family of bugs that metrics don't: wrong map, wrong key, expired value, failed insert, program attached in the wrong mode. It doesn't care why the block is absent. It only cares that the wire disagrees.

The instrumentation around it tells you *which* kind of wrong you're looking at: interface counters, `/proc/net/packet`, an independent AF_PACKET tap, a tcpdump cross-check, and a control tap on `lo` to separate "veth is weird" from "packet capture is broken machine-wide." That diagnostic ladder came out of a real investigation into a capture divergence, and it's the difference between a red build you can reason about and a red build you rerun until it goes green.

---

## Why this matters beyond DDoS

XDP is often filed under "DDoS mitigation," and it is very good at that. But the deeper idea is more interesting: it moves the first decision about a packet to the place where the packet is cheapest to reason about.

That's a security architecture principle, not a networking trick. Put the cheap check at the edge; put the expensive state where it belongs. Drop the obvious attacker at 40 bytes of packet data and 100 nanoseconds of CPU. Send everything ambiguous to the stack, the IDS, the application — with its full context intact.

The cost is discipline. The verifier demands it. The map contract demands it. The mode selection demands it. The proof demands it.

And the payoff is a system where a flood becomes a counter increment instead of an incident.

---

*RamShield is a Rust-based DDoS mitigation platform that puts XDP enforcement behind a WAL-durable, CRDT-replicated control plane. The code in this article is from the project's `ramshield-xdp` and `ramshield-enforcement` crates; the netns smoke gate lives in `scripts/xdp_netns_smoke.sh`.*
