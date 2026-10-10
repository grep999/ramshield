use super::*;

impl DetectionEngine {
    /// Test/IPC entry: aggregate raw events, then flush.
    pub fn flush_events(&self, events: &[ConnectionEvent]) {
        let a = aggregate(events);
        let aggs: Vec<(IpAddr, IpAgg)> = a.ips.into_iter().collect();
        // Synthetic batch: treat as a 1s window (caller-side tests assert counts, not rates).
        self.flush_batch(
            &aggs,
            &a.subnets,
            &a.networks,
            events.len() as u64,
            1_000_000_000,
        );
    }

    /// Single pass over aggregates: promote, merge, emit blocks. No store access for cold IPs.
    pub(crate) fn flush_batch(
        &self,
        ip_aggs: &[(IpAddr, IpAgg)],
        subnet_counts: &HashMap<SubnetKey, (u32, Vec<IpAddr>)>,
        networks: &HashMap<SubnetKey, IpNetwork>,
        total_events: u64,
        // F7: wall-clock ns span these events were collected over.
        window_ns: u64,
    ) {
        let cfg = self.config.load();
        let det = &cfg.detection;
        let ram_lim = cfg.engine.ram_limit_mb * 1024 * 1024;
        let now = now_ns();

        // Incremental counters for forecasting (no full-store scan).
        let subnet_vals: Vec<u64> = subnet_counts.values().map(|&(ev, _)| ev as u64).collect();
        // F7: events_last_second must be a RATE. Prod flushes every 100ms;
        // storing the raw per-flush count lied 10x low to the forecaster.
        let rate = total_events.saturating_mul(1_000_000_000) / window_ns;
        self.store
            .traffic
            .record_flush(rate, ip_aggs.len() as u64, &subnet_vals);

        // Dual-gate leg computed ONCE per subnet per flush (was: one
        // subnet_table read per IP — 256 reads of the same record per /24).
        // ponytail ceiling: frozen at Phase A (post-merge, pre-promotion);
        // merge_record side effects mid-loop no longer refresh it. Advisory
        // only: the in-batch legs use the same flush data, so any delta is
        // double-covered. Upgrade path: none expected — per-subnet here is
        // the correct granularity.
        let mut dual_gate_met: HashMap<SubnetKey, bool> =
            HashMap::with_capacity(subnet_counts.len());

        for (&sk, &(count, ref members)) in subnet_counts.iter() {
            let net = networks.get(&sk).copied().unwrap_or_else(|| {
                // pre-agg path passes no networks map — reconstruct the /24
                // (v4) or /64 (v6) network from the subnet key itself.
                // ponytail: v6 keys carry the full /64 in low 64 bits; a
                // from_key constructor on IpNetwork would avoid this branch.
                if sk <= 0xFFFF_FFFF {
                    let o = [
                        (sk >> 24) as u8,
                        (sk >> 16) as u8,
                        (sk >> 8) as u8,
                        sk as u8,
                    ];
                    IpNetwork::ipv4_subnet(std::net::Ipv4Addr::from(o))
                } else {
                    IpNetwork::ipv6_subnet(std::net::Ipv6Addr::from(sk))
                }
            });
            self.store
                .merge_subnet_window(sk, net, count, Some(members), now);
            // Hoisted dual gate: post-merge read, per subnet, not per IP.
            let dual = self.store.subnet_table().get(&sk).is_some_and(|r| {
                let uniq = if r.network.family() == 4 {
                    r.unique_ips()
                } else {
                    self.store
                        .subnet_member_count_windowed(sk, SUBNET_WINDOW_NS, now)
                };
                uniq >= det.subnet_batch_threshold as u64
                    && r.total_rps >= det.subnet_batch_min_events
            });
            dual_gate_met.insert(sk, dual);
        }

        let mut blocks = Vec::new();
        let mut promoted_ips: Vec<IpAddr> = Vec::with_capacity(64);
        let mut threat_sample = Vec::with_capacity(64);
        let mut promoted = 0u32;
        let mut cold_skipped = 0u32;
        let mut promoted_events = 0u32;
        let mut cold_skipped_events = 0u32;

        let unique_ips = ip_aggs.len();
        let hot_subnets = subnet_counts.len();

        for &(ip, ref agg) in ip_aggs {
            // L7 detection happens after bounded aggregation: request-level
            // telemetry never creates an unbounded side map or bypasses the
            // normal mitigation/WAL/XDP path.
            if det.l7_enabled && agg.l7_count > 0 {
                let l7_rps = agg.l7_count as u64 * 1_000_000_000 / window_ns.max(1);
                if l7_rps >= det.l7_rps_threshold {
                    self.emit_l7_block(ip, BlockReason::L7HighRps, det.l7_block_ttl_secs);
                }
                if agg.http2_opened >= det.l7_http2_min_streams {
                    let reset_pct = (agg.http2_reset as u64 * 100) / agg.http2_opened.max(1) as u64;
                    if reset_pct >= det.l7_http2_reset_ratio_pct as u64 {
                        self.emit_l7_block(
                            ip,
                            BlockReason::Http2StreamAbuse,
                            det.l7_block_ttl_secs,
                        );
                    }
                }
                for route in agg
                    .routes
                    .iter()
                    .filter(|r| r.route_hash != 0 && r.count > 0)
                {
                    for rule in &det.l7_rules {
                        if rule.route_hash != route.route_hash
                            || rule.method.is_some_and(|m| m != route.method)
                        {
                            continue;
                        }
                        let route_rps = route.count as u64 * 1_000_000_000 / window_ns.max(1);
                        let rate_hit = rule.max_rps.is_some_and(|max| route_rps >= max);
                        let latency_hit = rule
                            .max_latency_us
                            .is_some_and(|max| route.latency_max_us >= max);
                        let avg_latency_us = route.latency_sum_us / route.count.max(1) as u64;
                        let latency_ratio =
                            (avg_latency_us as f64 / rule.baseline_latency_us as f64).max(1.0);
                        let effective_rps = ((route_rps as f64) * rule.cost_weight * latency_ratio)
                            .ceil()
                            .min(u64::MAX as f64)
                            as u64;
                        let cost_hit = rule
                            .max_effective_rps
                            .is_some_and(|max| effective_rps >= max);
                        let h2_hit = rule.max_http2_reset_ratio_pct.is_some_and(|max| {
                            route.http2_opened >= det.l7_http2_min_streams
                                && route.http2_opened > 0
                                && (route.http2_reset as u64 * 100) / route.http2_opened as u64
                                    >= max as u64
                        });
                        if rate_hit || h2_hit || latency_hit || cost_hit {
                            let reason = if cost_hit || latency_hit {
                                BlockReason::L7Cost
                            } else if h2_hit {
                                BlockReason::Http2StreamAbuse
                            } else {
                                BlockReason::L7HighRps
                            };
                            self.emit_l7_block(ip, reason, det.l7_block_ttl_secs);
                            break;
                        }
                    }
                }
            }

            let sk = subnet_key_u128(ip);

            // Swarm hint (sparse /24 deadlock fix): Phase A above already
            // merged this batch's subnet window, so an IP with agg.count < 8
            // can still be part of an attack. Three independent legs:
            //   1. in-batch distinct hosts  — 60 IPs x 1 event is a swarm
            //   2. in-batch event volume    — matches the old subnet_hot leg
            //   3. cumulative store dual gate — swarm split across flushes
            // Without these, `agg.count < promote_min && !bloom_hit` dropped
            // every sparse host before the subnet aggregator ever saw it, so
            // subnet_hot could never become true (circular cold-skip).
            let (in_batch_events, in_batch_hosts) = sk
                .and_then(|k| subnet_counts.get(&k))
                .map(|(ev, members)| (*ev as u64, members.len() as u64))
                .unwrap_or((0, 0));

            let store_dual_gate_met = sk
                .map(|k| dual_gate_met.get(&k).copied().unwrap_or(false))
                .unwrap_or(false);

            let swarm_hint = in_batch_hosts >= det.subnet_batch_threshold as u64
                || in_batch_events >= det.subnet_window_threshold
                || store_dual_gate_met;

            // ponytail-ish: bloom consulted only for genuinely cold IPs
            // (count < promote_min && no swarm context) — for promoted or
            // swarm-covered IPs it is dead work (2 hashes + 2 loads per IP
            // per flush).
            if agg.count < det.promote_min_events && !swarm_hint {
                let (a, b) = BloomFilter::slots(&ip);
                let bloom_hit = self.bloom.load().contains_hashed(a, b);
                if !bloom_hit {
                    cold_skipped += 1;
                    cold_skipped_events += agg.count;
                    trace!(
                        ip = %ip,
                        events = agg.count,
                        bytes = agg.bytes,
                        proto_fp = agg.proto_fp,
                        in_batch_hosts,
                        in_batch_events,
                        store_dual_gate_met,
                        cold_skipped,
                        "ip cold-skipped: below promote gate"
                    );
                    continue;
                }
            }

            // ponytail: merge_record does the single store lookup (is_blocked check
            // was a second DashMap hit on the same key).
            let (_ewma_rps, threat, should_block, _was_blocked, stored) =
                self.merge_record(ip, agg, det, ram_lim, now, sk);
            if !stored {
                let exceeded = self
                    .metrics
                    .capacity_exceeded_count
                    .fetch_add(1, Ordering::Relaxed)
                    + 1;
                self.metrics
                    .capacity_exceeded_ips
                    .fetch_add(1, Ordering::Relaxed);
                // Sampled like the enforcement-queue warn below: under RAM
                // pressure this fires per refused IP and floods the log.
                if exceeded & 0x3FF == 1 {
                    warn!(
                        ip = %ip,
                        exceeded,
                        ram_usage = self.store.ram_bytes(),
                        ram_limit = ram_lim,
                        "detection: capacity-exceeded, IP not tracked (sampled 1/1024)"
                    );
                }
                cold_skipped += 1;
                cold_skipped_events += agg.count;
                continue;
            }
            // Note: we do NOT skip already-blocked IPs here. The pulse-wave
            // tracker needs to keep running on every batch to count distinct
            // over-threshold samples; subsequent bursts must still record
            // their state. The enforcement layer deduplicates block commands
            // by (ip, reason), so duplicate emits just refresh the TTL.

            promoted += 1;
            promoted_events += agg.count;
            // Patch A: the bloom is an advisory revisit cache over PROMOTED
            // IPs. Recording here (after merge_record succeeded) means a
            // promote-without-block host — the common case — is still
            // revisitable next epoch, which is what the cache is for.
            promoted_ips.push(ip);
            trace!(
                ip = %ip,
                events = agg.count,
                bytes = agg.bytes,
                threat,
                should_block,
                promoted,
                "ip promoted to store: gate passed"
            );

            if threat > 0.5 {
                threat_sample.push((ip, threat));
            }

            if should_block {
                // ponytail: debounce removed single-sample is_exceeded bypass
                blocks.push((ip, BlockReason::HighRps, det.block_ttl_secs));
            }
        }

        // Batch bloom insert — shared atomic words, no clone, no store.
        // (was: ArcSwap clone+insert+store — a 12.5 KB..1 MB memcpy per
        // flush at default bloom_bits, once per flush with any promote.)
        // Patch A: the insert set is PROMOTED IPs, not blocked ones. The
        // bloom is a revisit cache whose whole purpose is to let a host seen
        // last epoch skip the cold path this epoch. Blocked IPs are already
        // resident in the store carrying a BlockState, so caching them
        // duplicates state the store owns — while promote-without-block,
        // the common case, left the cache cold and made cold-skip fire on
        // hosts that had just been seen. Over-promoting is the intended
        // failure mode: a bloom FP opens the gate, never rejects.
        if !promoted_ips.is_empty() {
            let bf = self.bloom.load();
            for &ip in &promoted_ips {
                bf.insert_shared(ip);
            }
        }
        // n for the FP estimate: distinct promotes this epoch. Approximate
        // across flushes (an IP promoted in two flushes counts twice), which
        // only makes the reported FP conservative — it never understates fill.
        self.metrics.record_bloom_inserts(promoted_ips.len() as u64);
        threat_sample.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        threat_sample.truncate(128);
        self.store.traffic.push_threat_samples(threat_sample);
        self.store
            .traffic
            .promoted_ips
            .store(promoted as u64, Ordering::Relaxed);

        // ponytail: block metrics + enforcement send are fused into one loop
        // so the dashboard never counts a block the enforcement channel
        // dropped.  `blocks` in BatchRecord is the SENT count, not the
        // proposed count.
        let mut sent_blocks = 0u32;
        // ponytail: warn once per 1024 rejections — log churn kills throughput
        // under sustained queue pressure.
        let mut rejected = 0u32;
        // Bound the gate: only entry AGE is used (cooldown < ttl/2, and the
        // re-admission is what refreshes the block). Trim only under
        // attacker-cardinality overflow — 1M+ distinct (ip, reason) pairs.
        // ponytail: coarse 1h age prune on overflow; per-entry expiry only
        // if this gate ever shows up in a flame graph.
        if self.pending_mitigations.len() > 1_048_576 {
            self.pending_mitigations
                .retain(|_, ts| now.saturating_sub(*ts) < 3_600_000_000_000);
        }
        for b in blocks {
            let key = (b.0, b.1);
            if !self.admit_mitigation(key, b.2, now) {
                continue;
            }
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "detection".into(),
                actor: "system".into(),
                timestamp_utc: (now / 1_000_000_000) as i64,
                ttl_seconds: b.2,
                reason: b.1.as_str().into(),
                ip: b.0,
                cidr: None,
                action: EnforceAction::Block,
                evidence_source: ramshield_types::EvidenceSource::LocalSignals,
            };
            if self.enforcement_tx.try_send(cmd).is_ok() {
                sent_blocks += 1;
                self.metrics
                    .record_block_ip(&b.0, b.1.as_str(), "detection");
            } else {
                // Queue rejection is a delivery failure: undo the admission
                // so the NEXT window retries this (ip, reason).
                self.retreat_mitigation(key);
                rejected += 1;
                self.metrics.inc_enforcement_dropped();
                if rejected & 0x3FF == 1 {
                    warn!(ip=%b.0, rejected, "enforcement queue full; dropping {} block commands (sampled warn)", rejected);
                }
            }
        }

        self.metrics.record_batch(ramshield_metrics::BatchRecord {
            ts_ms: now / 1_000_000,
            events: total_events as u32,
            unique_ips: unique_ips as u32,
            promoted,
            cold_skipped,
            promoted_events,
            cold_skipped_events,
            blocks: sent_blocks,
            hot_subnets: hot_subnets as u32,
        });

        debug!(
            batch_id = now / 1_000_000,
            events = total_events,
            unique_ips,
            promoted_events,
            cold_skipped_events,
            blocks = sent_blocks,
            hot_subnets,
            "batch flush",
        );
    }
}
