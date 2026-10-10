use crate::*;

impl Store {
    /// Merge subnet-scale counters from a batch flush (O(subnets in batch)).
    /// Windowed: entries older than `window_ns` reset before adding, so a /24
    /// can't accumulate across windows and false-positive the batch blocker.
    pub fn merge_subnet_window(
        &self,
        key: SubnetKey,
        net: IpNetwork,
        events: u32,
        members: Option<&[std::net::IpAddr]>,
        now_ns: u64,
    ) {
        const WINDOW_NS: u64 = SUBNET_WINDOW_NS; // Patch B: one owner — see the const's doc for the 2s->4s rationale.
        // P0 fix: hold the shard lock for the full read-modify-write.
        // The old get().map(|e| e.value().clone()) → mutate → insert pattern
        // dropped the lock between read and write, so two concurrent
        // callers for the same subnet would both see the same baseline,
        // both compute stale deltas, and lose one update.
        self.subnet_table
            .entry(key)
            .and_modify(|rec| {
                // NTP step guard: the clock regressed (wall `now_ns()` is
                // non-monotonic). Never let a stepped-back timestamp set the
                // baseline — elapsed stays 0 (no early reset, no freeze),
                // events still accumulate, and the high-water baseline keeps
                // the window expiring on real forward time.
                let now_ns = now_ns.max(rec.last_updated_ns);
                if now_ns.saturating_sub(rec.last_updated_ns) > WINDOW_NS {
                    rec.total_rps = 0;
                    rec.host_bitmap = [0; 4];
                }
                rec.total_rps = rec.total_rps.saturating_add(events as u64);
                if let Some(ms) = members {
                    for ip in ms {
                        rec.mark_host_v4(*ip);
                    }
                }
                rec.last_updated_ns = now_ns;
            })
            .or_insert_with(|| {
                let mut rec = SubnetRecord {
                    network: net,
                    total_rps: 0,
                    host_bitmap: [0; 4],
                    last_updated_ns: now_ns,
                };
                rec.total_rps = rec.total_rps.saturating_add(events as u64);
                if let Some(ms) = members {
                    for ip in ms {
                        rec.mark_host_v4(*ip);
                    }
                }
                rec
            });
    }

    pub fn reset_subnet_window(&self, key: SubnetKey) {
        if let Some(mut e) = self.subnet_table.get_mut(&key) {
            e.total_rps = 0;
            e.host_bitmap = [0; 4];
        }
    }

    /// CIDR string for a subnet key ("198.51.100.0/24", "2001:db8::/64").
    /// Task 1: single display source for dashboard + batch-block logs.
    /// Empty string for unknown keys (log path only, never gates).
    pub fn subnet_cidr(&self, key: SubnetKey) -> String {
        self.subnet_table
            .get(&key)
            .map_or(String::new(), |e| e.network.to_string())
    }

    /// Exact distinct-host count for a subnet (IPv6 plan Task 2, G1/D1).
    /// Reads the reverse index (`subnet_index`), which every store insert
    /// and eviction already maintains for BOTH families — so v6 /64s, which
    /// no bitmap can cover, get exact cardinality for free.
    /// ponytail ceiling: index membership lives until store eviction, not
    /// the 2s gate window. Acceptable because the second gate leg
    /// (`total_rps`) IS windowed, so a stale swarm can't pass both. Upgrade
    /// path if v6 false positives appear: per-subnet last-seen window.
    pub fn subnet_member_count(&self, key: SubnetKey) -> u64 {
        self.subnet_index
            .get(&key)
            .map_or(0, |ips| ips.len() as u64)
    }

    /// Windowed variant used by the batch gate for v6: counts only members
    /// whose store record was seen within `window_ns` of `now_ns`. The raw
    /// index counts LIFETIME hosts (never pruned on unblock), so without
    /// this a cooled-off /64 with 60 historical members plus a tiny fresh
    /// burst (5 hosts, 100 events) passes the dual gate and batch-blocks
    /// ~55 innocent hosts. O(members) per call — acceptable: gates run on
    /// flush cadence, subnets are small.
    pub fn subnet_member_count_windowed(&self, key: SubnetKey, window_ns: u64, now_ns: u64) -> u64 {
        self.subnet_index.get(&key).map_or(0, |ips| {
            ips.iter()
                .filter(|ip| {
                    self.inner.get(*ip).is_some_and(|v| {
                        let ls = match &v.value().value {
                            Value::IpRecord(rec) => rec.last_seen_ns,
                            _ => 0,
                        };
                        now_ns.saturating_sub(ls) <= window_ns
                    })
                })
                .count() as u64
        })
    }

    /// Update the reverse index for subnet lookups. Call after inserting/updating an IP record.
    pub const MAX_SUBNET_INDEX_KEYS: usize = 131_072;

    pub fn update_subnet_index(
        &self,
        ip_key: IpAddr,
        subnet_key: Option<SubnetKey>,
        is_removal: bool,
    ) {
        let Some(sk) = subnet_key else { return };

        if is_removal {
            // P0 fix: the old path cloned the inner DashSet
            // (DashSet::clone is a DEEP clone, not an Arc handle), removed the
            // IP from the throwaway copy, and emptiness-tested the copy too.
            // The real index entry was NEVER modified — every evicted/unblocked
            // IP leaked into subnet_index forever, and subnet batch-block kept
            // returning dead hosts for subnet batch-block. Removal is now a
            // conditional remove_if on the live entry: removes the IP, drops
            // the subnet key only if the set went empty, and a concurrent
            // insert into the same set makes the predicate false so the key
            // stays. One shard lock per step, no TOCTOU.
            use dashmap::mapref::entry::Entry;
            if let Entry::Occupied(mut e) = self.subnet_index.entry(sk) {
                e.get_mut().remove(&ip_key);
                if e.get().is_empty() {
                    e.remove(); // consumes the entry; shard lock held throughout
                }
            }
        } else {
            // Hard cardinality ceiling: IPv6 prefix hopping can manufacture an
            // unbounded number of /64 keys. Detection continues to operate, but
            // the reverse index refuses new keys once bounded capacity is reached.
            if !self.subnet_index.contains_key(&sk)
                && self.subnet_index.len() >= Self::MAX_SUBNET_INDEX_KEYS
            {
                return;
            }
            self.subnet_index
                .entry(sk)
                .or_insert_with(
                    || std::collections::HashSet::with_hasher(ahash::RandomState::new()),
                )
                .insert(ip_key);
        }
    }
}
