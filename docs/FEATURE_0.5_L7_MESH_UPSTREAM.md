# RamShield 0.5 — L7, HTTP/2, mesh and upstream integration

This release closes the four architectural limitations without replacing the
existing detection → WAL → EnforcementService → XDP pipeline.

## 1. L7 / HTTP/2 telemetry

Existing `ConnectionEvent` now accepts optional bounded `L7Metadata`:

- `host_hash`
- `route_hash`
- HTTP method
- HTTP version
- latency
- request/response bytes
- HTTP/2 stream counters

No raw URLs, headers or request bodies are stored by the detector.

The existing IPC `report_connection` / `report_connections` messages accept
`l7` as an optional field. Old clients remain wire-compatible because the
field is omitted when absent.

### HTTP/2 signals

RamShield scores:

- request rate;
- streams opened;
- streams reset;
- reset ratio;
- completed streams;
- maximum active streams.

The detector blocks through the existing `EnforcementService`; it does not
implement an HTTP/2 parser. A terminating proxy supplies the counters.

### Route rules

A bounded `detection.l7_rules` list can match a producer-side `route_hash`
and optional HTTP method. Rules can constrain request rate and/or HTTP/2 reset
ratio. At most 64 rules are accepted.

Example:

```toml
[detection]
l7_enabled = true
l7_rps_threshold = 500
l7_http2_min_streams = 32
l7_http2_reset_ratio_pct = 80
l7_block_ttl_secs = 300

[[detection.l7_rules]]
route_hash = 123456789
method = "Post"
max_rps = 50
max_http2_reset_ratio_pct = 70
```

## 2. Multi-node mesh

The existing `ramshield-mesh` AWORSet is now wired into the existing
`EnforcementService`.

When enabled:

```text
local block
  -> existing WAL/store/XDP
  -> existing AWORSet
  -> authenticated mesh delta
  -> peer AWORSet
  -> existing EnforcementService
```

Transport is authenticated TCP with an HMAC-SHA256 envelope and a 30-second
clock-skew/replay window. A bounded anti-entropy snapshot is exchanged every
2 seconds so a temporarily unavailable peer can converge after reconnect.

Remote tombstones are allowed to release only blocks that were applied from
mesh state; operator/local blocks are never blindly removed by a remote
unblock.

Configuration:

```toml
[mesh]
enabled = true
node_id = 1
listen_addr = "10.0.0.11:7900"
peers = ["10.0.0.12:7900", "10.0.0.13:7900"]
peer_node_ids = [2, 3]
peer_auth_keys = ["<edge-1-2-hex-secret>", "<edge-1-3-hex-secret>"]
auth_key = ""
```

Each configured edge has its own shared HMAC secret. `peer_auth_keys[i]` is the
secret of the edge between this node and the peer at `peers[i]`, and **both
ends of that edge must configure the identical secret**. The secrets of two
different edges on one node must differ, and every secret must decode to at
least 16 bytes.

`auth_key` is retained for configuration compatibility only. Mesh frames and
application ACKs are signed and verified with the per-edge secrets, so it may
be left empty. It is no longer a local signing key, and it does not have to
differ from an edge secret.

**Key provisioning.** Node 1 above and node 2 must both hold the same secret
for the 1-2 edge; node 1 and node 3 must both hold the same secret for the
1-3 edge. A secret for one edge must not be provisioned to a node that is not
an endpoint of that edge. This is a key-distribution change from earlier
releases, where every node held a private key and each of its peers held a
copy of it: provision the shared secret at both endpoints of every edge
before enabling the mesh.

**What the guarantee is, and is not.** A valid MAC proves possession of the
edge secret. It does not prove which of the two endpoints produced the frame,
because both hold the same secret. Inbound frames are additionally bound to
the source IP address configured for the claimed peer, so attribution relies
on that binding together with the confidentiality of the edge secret. A node
that captures an edge secret can authenticate on that edge only; it cannot
forge traffic for an edge it is not an endpoint of.

The example values are placeholders, not usable credentials.

## 3. Uplink saturation / upstream escalation

The existing host detector cannot prevent an attack that has already filled
the access link. 0.5 therefore adds an upstream escalation signal while
leaving local enforcement untouched.

When configured, RamShield measures RX throughput on the configured interface
and compares it with the declared link capacity. A signal is raised when:

```text
uplink utilization >= saturation_pct
AND
observed request rate >= 50% of detection.rps_threshold
```

The signal is logged and, if configured, delivered to a generic HTTP webhook.
The webhook is deliberately vendor-neutral: it can invoke a provider's
scrubbing, RTBH or FlowSpec automation without putting BGP credentials inside
RamShield.

```toml
[upstream]
enabled = true
interface = "eth0"
link_capacity_mbps = 1000
saturation_pct = 90
poll_ms = 1000
webhook_url = "http://127.0.0.1:8787/ramshield/mitigate"
cooldown_secs = 60
```

The current adapter deliberately accepts `http://` only; production TLS can
be terminated by the deployment's existing local proxy. This avoids adding a
second TLS stack to the daemon.

## 4. Cross-platform boundary

The core telemetry, protocol, detection and userspace enforcement path remains
independent of XDP. XDP is still Linux-only and remains optional. Non-Linux
clients can therefore participate through the existing IPC telemetry and
control surface; Linux retains the kernel dataplane where available.

No Windows kernel firewall implementation is introduced into this release.
That is intentional: the project gains cross-platform participation without
creating a second kernel enforcement implementation.

## Integrity rules

These features must not:

- bypass WAL;
- mutate Store outside `EnforcementService`;
- write XDP directly from detection;
- introduce an unbounded per-route map;
- trust unauthenticated mesh messages;
- let mesh unblocks remove local/manual blocks;
- claim that host-level mitigation defeats a saturated uplink.
