use super::*;

#[derive(Debug, Clone, Default)]
pub struct ReconciliationState {
    pub last_wal_lsn: u64,
    pub pending_blocks: Vec<IpAddr>,
    pub pending_unblocks: Vec<IpAddr>,
    /// Stale entries removed from kernel maps during reconcile (proxy for LRU
    /// eviction pressure — userspace cannot observe kernel-side LRU drops).
    pub evicted_count: u64,
}

#[async_trait::async_trait]
pub trait XdpApplier: Send + Sync {
    /// ttl_seconds: 0 = permanent block (u64::MAX expiry in the map).
    fn apply_block(
        &mut self,
        ip: IpAddr,
        decision_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<(), EnforcementError>;
    fn apply_unblock(&mut self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    fn apply_cidr_block(
        &mut self,
        network: IpNetwork,
        _decision_id: Uuid,
        _ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        Err(EnforcementError::Xdp(format!(
            "CIDR enforcement unsupported: {network}"
        )))
    }
    fn apply_cidr_unblock(
        &mut self,
        network: IpNetwork,
        _decision_id: Uuid,
    ) -> Result<(), EnforcementError> {
        Err(EnforcementError::Xdp(format!(
            "CIDR enforcement unsupported: {network}"
        )))
    }
    /// Configure trusted overlay CIDR blocklist (IPv4/IPv6 LPM trie maps).
    fn configure_trusted_overlay(
        &mut self,
        _cidrs: &[IpNetwork],
    ) -> Result<(), EnforcementError> {
        // Keep the optional overlay hook a no-op for appliers without a kernel overlay map.
        Ok(())
    }
    /// Configure autonomous mode (syn/udp/packet PPS per-CPU limits).
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        // Keep the optional autonomous hook a no-op for appliers without kernel controls.
        Ok(())
    }
    /// Reconcile both per-IP hash maps and CIDR LPM-trie maps against the
    /// userspace source of truth. CIDRs are included explicitly because they
    /// are not represented by `Store::get_all_blocked_ips()`.
    fn reconcile(
        &mut self,
        expected_blocks: &[IpAddr],
        expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError>;
    /// Drain kernel→userspace drop notifications (RingBuf). Default: no channel.
    fn drain_drop_events(&mut self) -> Vec<XdpDropEvent> {
        Vec::new()
    }
    fn counters(&mut self) -> Result<[u64; 4], EnforcementError> {
        Ok([0; 4])
    }
}

/// One kernel→userspace drop notification from the XDP EVENTS ringbuf.
/// `ip` = dropped source address, `ts_ns` = monotonic clock (bpf_ktime_get_ns),
/// `slot` = COUNTERS slot that was incremented (0 = v4_drop, 1 = v6_drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XdpDropEvent {
    pub ip: IpAddr,
    pub ts_ns: u64,
    pub slot: u8,
}

pub struct StubXdpApplier;

#[async_trait::async_trait]
impl XdpApplier for StubXdpApplier {
    fn apply_block(
        &mut self,
        ip: IpAddr,
        _decision_id: Uuid,
        _ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        trace!(%ip, "XDP block (stub)");
        Ok(())
    }
    fn apply_unblock(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        trace!(%ip, "XDP unblock (stub)");
        Ok(())
    }
    fn configure_trusted_overlay(&mut self, _cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn reconcile(
        &mut self,
        _expected_blocks: &[IpAddr],
        _expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count: 0,
        })
    }
}
