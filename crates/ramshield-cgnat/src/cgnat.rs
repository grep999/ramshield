//! Deterministic CGNAT guard for subnet-level mitigation.

use std::net::IpAddr;

pub const CGNAT_TIER_ALLOW: u8 = 0;
pub const CGNAT_TIER_CHALLENGE: u8 = 1;
pub const CGNAT_TIER_XDP_DROP: u8 = 2;
pub const CGNAT_TIER_BLOCK: u8 = 3;

pub struct CgnatGuard;

impl Default for CgnatGuard {
    fn default() -> Self {
        Self
    }
}

impl CgnatGuard {
    pub fn new() -> Self {
        Self
    }

    pub fn is_shared_infrastructure(ip: IpAddr) -> bool {
        matches!(ip, IpAddr::V4(v4) if (u32::from(v4) & 0xffc0_0000) == 0x6440_0000)
    }

    /// Classify using network identity, active-host density, and window rate.
    pub fn classify_subnet(&self, subnet: IpAddr, host_count: u64, rate: u64) -> u8 {
        match (Self::is_shared_infrastructure(subnet), host_count, rate) {
            (false, hosts, r) if hosts > 10 && r > 100 => CGNAT_TIER_BLOCK,
            (true, _, r) if r > 100_000 => CGNAT_TIER_CHALLENGE,
            (_, _, r) if r >= 5_000 => CGNAT_TIER_CHALLENGE,
            _ => CGNAT_TIER_ALLOW,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn classifier_uses_rfc6598_density_and_rate() {
        let guard = CgnatGuard::new();
        let cgnat = IpAddr::V4(Ipv4Addr::new(100, 64, 1, 0));
        let public = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 0));
        assert_eq!(
            guard.classify_subnet(cgnat, 10, 100_001),
            CGNAT_TIER_CHALLENGE
        );
        assert_eq!(
            guard.classify_subnet(cgnat, 10, 5_001),
            CGNAT_TIER_CHALLENGE
        );
        assert_eq!(guard.classify_subnet(public, 65, 50_001), CGNAT_TIER_BLOCK);
        assert_eq!(
            guard.classify_subnet(public, 10, 5_000),
            CGNAT_TIER_CHALLENGE
        );
    }
}
