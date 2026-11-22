# RamShield BGP Implementation Design — PhD-Level Deep Dive

## 1. BGP-4 Protocol Internals (RFC 4271)

### 1.1 Message Format
```
BGP Header (19 octets minimum):
  Marker: 16 octets (all 1s for authentication)
  Length: 2 octets (19-4096)
  Type: 1 octet (1=OPEN, 2=UPDATE, 3=NOTIFICATION, 4=KEEPALIVE)
```

### 1.2 BGP FSM (Finite State Machine) — 6 States
```
Idle → Connect → OpenSent → OpenConfirm → Established
                    ↓              ↓
                   Idle ←←←←←←←←←←←←←← (any error)
```
**State session attributes (mandatory):**
- State, ConnectRetryCounter, ConnectRetryTimer, ConnectRetryTime
- HoldTimer, HoldTime, KeepaliveTimer, KeepaliveTime

**Optional session attributes:**
- DampPeerOscillations, IdleHoldTime, IdleHoldTimer, DelayOpen, PassiveTcpEstablishment, CollisionDetectEstablishedState

### 1.3 Path Attributes (UPDATE message)
Each attribute: `<Attribute Flags (1 octet), Attribute Type Code (1 octet), Attribute Length (1-2 octets), Attribute Value (variable)>`

**Attribute Flags bits:**
- Bit 0 (Optional): 1=optional, 0=well-known
- Bit 1 (Transitive): 1=transitive, 0=non-transitive (well-known MUST be 1)
- Bit 2 (Partial): 1=partial info, 0=complete (well-known MUST be 0)
- Bit 3 (Extended Length): 1=2-octet length, 0=1-octet length
- Bits 4-7: Unused (must be zero)

**Well-known mandatory attributes (must appear in every UPDATE with NLRI):**
- ORIGIN (Type 1): IGP=0, EGP=1, INCOMPLETE=2
- AS_PATH (Type 2): Sequence of segments <type, length, value[]>
- NEXT_HOP (Type 3): Next-hop address

**Well-known discretionary:**
- LOCAL_PREF (Type 5), ATOMIC_AGGREGATE (Type 6)

**Optional transitive:**
- AGGREGATOR (Type 7), COMMUNITY (Type 8), EXTENDED_COMMUNITIES (Type 16)

---

## 2. Multiprotocol Extensions (RFC 4760) + Capabilities (RFC 5492)

### 2.1 MP_REACH_NLRI (Type 14) / MP_UNREACH_NLRI (Type 15)
Both are **optional non-transitive** attributes.

```
MP_REACH_NLRI encoding:
  AFI (2 octets) | SAFI (1 octet) | NextHop Length (1) | NextHop (variable) | Reserved (1) | NLRI (variable)
```

**Flowspec AFI/SAFI:**
- AFI=1 (IPv4), SAFI=133 (Flow Spec) or SAFI=134 (VPNv4 Flow Spec)
- AFI=2 (IPv6), SAFI=133 (IPv6 Flow Spec per RFC 8956)

### 2.2 Capability Advertisement (OPEN message optional parameter)
```
Capability Code: 1 (Multiprotocol Extensions)
Capability Length: 4
Capability Value: AFI (2) | Reserved (1) | SAFI (1)
```
Must be advertised by **both peers** for bi-directional exchange.

---

## 3. Flow Specification (RFC 8955/8956)

### 3.1 Flow Spec NLRI Components (IPv4)
**Component ordering: strict increasing numerical order (MANDATORY)**

| Type | Component | Encoding |
|------|-----------|----------|
| 1 | Destination Prefix | `<type(1), length(1), prefix(var)>` — same as BGP UPDATE |
| 2 | Source Prefix | `<type(1), length(1), prefix(var)>` |
| 3 | IP Protocol | `<type(1), length(1), op(1), value(var)>` — operator: 0x01=value, 0x02=mask, 0x03=range |
| 4 | Port | `<type(1), length(1), op(1), value(var)>` |
| 5 | Destination Port | same as Type 4 |
| 6 | Source Port | same as Type 4 |
| 7 | ICMP Type | `<type(1), length(1), op(1), value(var)>` |
| 8 | ICMP Code | same as Type 7 |
| 9 | TCP Flags | `<type(1), length(1), op(1), mask(1), value(1)>` — match if (flags & mask) == value |
| 10 | Packet Length | `<type(1), length(1), op(1), value(var)>` |
| 11 | DSCP | `<type(1), length(1), op(1), value(var)>` |
| 12 | Fragment | `<type(1), length(1), op(1), value(var)>` — 0=DF, 1=MF, 2=first frag |

**Operator encoding:**
- Bit 0 (e): 1=equality, 0=inequality
- Bit 1 (a): 1=AND mask, 0=OR mask
- Bits 2-4 (len): operand length
- Bit 5 (not): 1=NOT match
- Bit 6 (m): 1=mask operation
- Bit 7: reserved (0)

### 3.2 Flow Spec Actions (BGP Extended Communities — RFC 8955 Section 7)
Encoded as **Transitive Extended Community Type 0x07** (FlowSpec)

| Sub-Type | Action | Value Format |
|----------|--------|--------------|
| 0x01 | traffic-rate | 8 octets: rate (4) + burst (4) in bytes/sec |
| 0x02 | traffic-action | 1 octet: 0x01=drop, 0x02=sample, 0x03=redirect |
| 0x03 | redirect | IPv4: 4 octets next-hop; IPv6: 16 octets |
| 0x04 | traffic-marking | 4 octets: DSCP value |
| 0x05 | redirect-IP | Extended next-hop encoding |
| 0x08 | rt-redirect | AS-specific or IPv4/IPv6 format |
| 0x09 | traffic-remarking | DSCP remarking |

**Flow Spec ordering rule:** Two Flow Specs compared by component-by-component; first difference determines priority. More specific (longer prefix) wins. This ensures **consistent network-wide ordering** regardless of arrival order.

---

## 4. RTBH (RFC 7999) — Blackhole Community

```
Well-known transitive community: 65535:666 (0xFFFF:0x029A)
MUST be accompanied by NO_EXPORT (65535:65281) to prevent leakage
```

**Upstream router behavior on receiving BLACKHOLE community:**
1. Match prefix length ≤ /32 (IPv4) or ≤ /128 (IPv6)
2. Set next-hop to blackhole next-hop (e.g., 192.0.2.1 / 100::1)
3. Route to Null interface — hardware FIB, zero TCAM pressure
4. Apply NO_EXPORT to contain within AS

**Vendor implementation (Cisco IOS example):**
```
route-map v4blackhole-trigger permit 10
  match tag 666
  set local-preference 1000
  set origin igp
  set community no-export 65535:666
  set ip next-hop 192.0.2.1
```

---

## 5. Route Flap Dampening (RFC 2439 / RFC 7196)

### 5.1 Penalty Algorithm
```
Penalty(t) = Penalty(t-1) * e^(-λ * Δt) + PenaltyIncrement
λ = ln(2) / half-life
```
- **Withdrawal**: +1000 penalty
- **Re-advertisement**: +0 (Cisco) / +1000 (Juniper)
- **Attribute change**: +500
- **Half-life**: 15 min (default)
- **Reuse threshold**: 750 (below → re-advertise)
- **Suppress threshold**: 2000-3000 (above → suppress)
- **Max suppress time**: 60 min

### 5.2 RFC 7196 Recommendations (Making RFD Usable)
- **Router Maximum Penalty**: MUST be raised to ≥ 50,000
- **Suppress Threshold**: Conservative ≥ 12,000; Minimum ≥ 6,000
- **Calculate-but-do-not-damp** mode for testing

### 5.3 Anti-Flap Strategy for RamShield
**Problem:** Withdrawing RTBH when attack stops → link drops to 0% → traffic returns → re-announce → flap penalty → 1hr suppression.

**Solution (carrier-standard 15-minute hold):**
```
State machine:
  [Trigger BGP Block] → Hold 900s minimum
       │
       ├─ Attack still active? → Extend +900s
       └─ Clear after 900s? → Withdraw cleanly
```

**Flap penalty escalation:** Same IP attacked again within 1 hour → hold = 1800s (30 min).

**Schmitt Trigger Hysteresis (local):**
```
High watermark: 85% link utilization OR ring drops > 10k pps → Tier 2/3
Low watermark:  50% link utilization AND cooldown elapsed → Tier 1
Cooldown: base_cooldown * 2^flap_count (capped at 5)
```

---

## 6. BGP Extended Communities (RFC 4360 / RFC 7153 / RFC 8955)

### 6.1 Structure (8 octets)
```
Type Field (1-2 octets):  IANA bit (1) | Transitive bit (1) | 6-bit structure
Value Field: remaining octets
```

**FlowSpec Transitive Extended Community (Type 0x07):**
- Sub-types 0x01-0x09 as defined in Section 3.2

**Encoding example (traffic-rate drop):**
```
0x07 0x01 [rate:4bytes] [burst:4bytes]  (transitive, sub-type 1)
```

### 6.2 Large Communities (RFC 8092) — 12 octets
```
Global Administrator (4) | Local Data Part 1 (4) | Local Data Part 2 (4)
```
Useful for carrier-specific RTBH communities (e.g., Cogent 174:666).

---

## 7. RamShield BGP Engine Architecture

### 7.1 Native Micro-BGP Speaker (Option 1)
**Minimal FSM implementation — only 4 message types:**

```rust
// BGP Header
struct BgpHeader {
    marker: [u8; 16],
    length: u16,
    msg_type: u8,
}

// OPEN Message
struct OpenMessage {
    version: u8,           // 4
    my_asn: u16,           // local_asn
    hold_time: u16,        // 90 (30s keepalive * 3)
    bgp_id: u32,           // router_id
    opt_params: Vec<OptParam>, // Capabilities
}

// UPDATE Message (FlowSpec / RTBH)
struct UpdateMessage {
    withdrawn_routes: Vec<IpNet>,      // empty for announcement
    path_attributes: Vec<PathAttr>,    // ORIGIN, AS_PATH, NEXT_HOP, COMMUNITIES, MP_REACH_NLRI
    nlri: Vec<IpNet>,                  // for RTBH: /32; for Flowspec: empty (in MP_REACH)
    mp_reach_nlri: Option<MpReachNlri>, // for Flowspec
}

// NOTIFICATION
struct Notification {
    error_code: u8,
    error_subcode: u8,
    data: Vec<u8>,
}
```

### 7.2 BGP Session State Machine (simplified)
```
Idle --(TCP connect)--> Connect --(OPEN sent)--> OpenSent
OpenSent --(OPEN received, valid)--> OpenConfirm --(KEEPALIVE)--> Established
Established --(UPDATE sent)--> [FlowSpec/RTBH announced]
Established --(HoldTimer expiry)--> Idle
Any error --> NOTIFICATION --> Idle
```

### 7.3 Flowspec NLRI Encoder (RFC 8955 strict ordering)
```rust
fn encode_flowspec_nlri(components: &[FlowComponent]) -> Vec<u8> {
    // Components MUST be sorted by type (1..12)
    let mut nlri = Vec::new();
    for comp in components.iter().filter(|c| c.is_valid()) {
        nlri.push(comp.component_type);
        nlri.extend(comp.encode());
    }
    nlri
}

// Example: UDP/53 reflection filter for 198.51.100.42/32
components = [
    Type1: DestPrefix(198.51.100.42/32),
    Type3: IPProtocol(17=UDP),
    Type6: SourcePort(53),  // DNS reflection source port
]
```

### 7.3 Integration with Schmitt Trigger Controller
```rust
pub struct BgpController {
    session: BgpSession,
    controller: HysteresisController,
    config: BgpConfig,
}

impl BgpController {
    pub fn evaluate(&mut self, metrics: InterfaceMetrics) -> Vec<BgpAction> {
        let action = self.controller.evaluate(metrics.rx_bps, metrics.ring_drops);
        match action {
            TransitionAction::AssertUpstreamRules => {
                if self.config.flowspec.enabled && metrics.is_udp_reflection() {
                    vec![BgpAction::AnnounceFlowspec(self.encode_udp_reflection_flowspec())]
                } else {
                    vec![BgpAction::AnnounceRtbh(self.config.local_ip)]
                }
            }
            TransitionAction::WithdrawUpstreamRules => {
                vec![BgpAction::WithdrawAll]
            }
            TransitionAction::Maintain => vec![],
        }
    }
}
```

---

## 8. Competitive Analysis: Why This Beats Enterprise

| Layer | Enterprise (Arbor/FastNetMon) | RamShield |
|-------|------------------------------|-----------|
| **Telemetry** | Sampled NetFlow/sFlow (1:1000-10000) | Full line-rate XDP/eBPF (1:1) |
| **Detection** | 15-45s (flow buffer flush) | <500ms (direct counter breach) |
| **BGP Mitigation** | 20-60s (collector → controller → BGP) | <1.5s (local socket write) |
| **TCAM Impact** | Multi-rule push → session reset risk | Single RTBH + surgical Flowspec |
| **Dependencies** | DB + collectors + external BGP | Single binary or Unix socket to BIRD |

---

## 9. Implementation Files (Target)

```
crates/ramshield-bgp/
├── src/
│   ├── lib.rs              # Public API
│   ├── fsm.rs              # BGP FSM (6 states)
│   ├── messages.rs         # OPEN/UPDATE/KEEPALIVE/NOTIFICATION
│   ├── attributes.rs       # Path attributes, communities, MP_NLRI
│   ├── flowspec.rs         # RFC 8955/8956 encoder/decoder
│   ├── rtbh.rs             # RFC 7999 blackhole community
│   ├── session.rs          # TCP/179 session management
│   ├── controller.rs       # Schmitt trigger integration
│   └── config.rs           # TOML config parsing
├── tests/
│   ├── fsm_test.rs
│   ├── flowspec_encoding_test.rs
│   └── rtbh_test.rs
└── benches/
    └── encode_bench.rs
```

---

## 10. References (Authoritative)

- **RFC 4271** — BGP-4 (core protocol, FSM, attributes)
- **RFC 4760** — Multiprotocol Extensions (AFI/SAFI, MP_REACH_NLRI)
- **RFC 5492** — Capabilities Advertisement
- **RFC 1997** — BGP Communities (NO_EXPORT, NO_ADVERTISE)
- **RFC 4360** — BGP Extended Communities (structure, transitive bit)
- **RFC 7153** — IANA Registries for Extended Communities
- **RFC 7999** — BLACKHOLE Community (RTBH)
- **RFC 8955** — Flow Specification (IPv4)
- **RFC 8956** — Flow Specification (IPv6)
- **RFC 2439** — Route Flap Dampening (original)
- **RFC 7196** — Making RFD Usable (parameter recommendations)
- **RFC 8092** — Large Communities
- **NANOG 30** — Chris Morrow RTBH presentation (de facto operational guide)

---

## 11. Key Implementation Decisions (ponytail: ceilings)

| Decision | Rationale | Ceiling | Upgrade Path |
|----------|-----------|---------|--------------|
| Native micro-speaker over BIRD/ExaBGP | Zero deps, single binary | No route reflection, no iBGP | Add iBGP client if multi-hop needed |
| Flowspec only for UDP reflection | 80% of volumetric DDoS | No TCP/ICMP Flowspec | Add component types as needed |
| 15-min hold timer | Carrier standard, avoids RFD | Fixed, not adaptive per-prefix | Per-prefix hold based on attack history |
| Strict component ordering | RFC 8955 mandates network-wide consistency | None | None — mandatory |
| NO_EXPORT always added | Prevents blackhole leakage beyond transit | None — security requirement | None |

---

*This design implements the minimal, orthodox BGP surface that every Tier-1 carrier supports natively in hardware. No route import, no FIB manipulation, no complex policy — just a signaling gun that fires pre-formatted byte buffers when the kernel says "link saturated".*