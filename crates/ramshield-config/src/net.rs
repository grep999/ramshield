use super::*;

/// True when `peer` is in `trusted` (exact IP or CIDR member).
/// No new deps — std IpAddr bit-math only. Invalid entries never match.
pub fn peer_is_trusted_proxy(peer: IpAddr, trusted: &[String]) -> bool {
    trusted.iter().any(|e| cidr_contains(e, peer))
}

/// Extract the real client IP from an X-Forwarded-For header value by walking
/// right-to-left and skipping trusted proxies. Returns None when absent,
/// unparseable, or all entries are trusted (caller falls back to peer).
pub fn xff_client(header: Option<&str>, trusted: &[String]) -> Option<IpAddr> {
    let header = header?;
    // Join is already done by caller; walk right-to-left.
    for entry in header.split(',').rev() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let ip = match entry.parse::<IpAddr>() {
            Ok(i) => i,
            // Fail closed: a non-empty malformed hop terminates the trust walk.
            // Continuing left could expose an attacker-controlled address past
            // a broken link in the chain. Caller falls back to the direct peer.
            Err(_) => return None,
        };
        // Skip if this hop is a trusted proxy; the next one left is the real client.
        if peer_is_trusted_proxy(ip, trusted) {
            continue;
        }
        // First untrusted hop from the right is the original client.
        return Some(ip);
    }
    None
}

fn cidr_contains(entry: &str, peer: IpAddr) -> bool {
    let entry = entry.trim();
    if !entry.contains('/') {
        return entry.parse::<IpAddr>().ok() == Some(peer);
    }
    let (net, prefix) = match entry.split_once('/') {
        Some(p) => p,
        None => return false,
    };
    let prefix: u32 = match prefix.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    match (net.trim().parse::<IpAddr>(), peer) {
        (Ok(IpAddr::V4(n)), IpAddr::V4(p)) if prefix <= 32 => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(n) & mask == u32::from(p) & mask
        }
        (Ok(IpAddr::V6(n)), IpAddr::V6(p)) if prefix <= 128 => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(n) & mask == u128::from(p) & mask
        }
        _ => false,
    }
}

/// True when `addr` binds an interface reachable from other hosts.
/// Loopback (127.0.0.1, ::1) and link-local (169.254.x.x, fe80::/10) are
/// host-private; all other L3 addresses — including RFC1918 private
/// ranges — are LAN-reachable and treated as public for fail-closed
/// exposure purposes. Hostnames are treated as private (DNS may resolve
/// anywhere; a wrong answer is a config bug, not a code risk).
pub(crate) fn is_public_bind(addr: &str) -> bool {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    let host = host.trim_matches(['[', ']']);
    if host.is_empty() || host == "0.0.0.0" || host == "::" || host == "*" {
        return true;
    }
    match host.parse::<IpAddr>() {
        // Loopback and link-local are unreachable from other hosts. Everything
        // else — including RFC1918 private ranges — is reachable from the LAN,
        // so an unauthenticated bind there is an exposure.
        Ok(IpAddr::V4(ip)) => !(ip.is_loopback() || ip.is_link_local()),
        Ok(IpAddr::V6(ip)) => !(ip.is_loopback() || ip.is_unicast_link_local()),
        Err(_) => {
            // ponytail: fail-CLOSED on unparseable hostnames. A hostname like
            // "dashboard.example.com" would otherwise fall through as "private"
            // and skip the public-bind-without-auth bail — an operator could
            // set `dashboard.http_addr = "dashboard.example.com:9999"` and
            // ship a public dashboard with no password. DNS may resolve to a
            // private address (and we can't tell here), but the safe default
            // is to require auth. Upgrade: resolve DNS at startup and cache.
            //
            // EXCEPTION: "localhost" (RFC 6761) is a reserved loopback name.
            // Treat it as non-public so dev workflows without auth still work.
            host != "localhost"
        }
    }
}

/// True when `addr`'s host is a loopback IP (127.0.0.0/8, ::1).
/// Browsers accept `Secure` cookies over plain HTTP only for loopback
/// origins (RFC 6265bis "trustworthy origin"). A non-loopback HTTP bind
/// without TLS makes the browser silently drop the `Secure` session
/// cookie → infinite login loop with no server-side error.
pub fn is_loopback_bind(addr: &str) -> bool {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    let host = host.trim_matches(['[', ']']);
    if host.is_empty() || host == "0.0.0.0" || host == "::" || host == "*" {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(IpAddr::V6(ip)) => ip.is_loopback(),
        // Hostnames: can't classify statically — treat as non-loopback so
        // the warning fires (a loopback hostname like "localhost" gets a
        // harmless extra warning; a LAN hostname missing the warning is
        // the dangerous direction).
        Err(_) => false,
    }
}

#[cfg(test)]
mod xff_tests {
    use super::*;

    #[test]
    fn no_header_returns_none() {
        assert_eq!(xff_client(None, &[]), None);
    }

    #[test]
    fn single_untrusted_peer_is_client() {
        assert_eq!(
            xff_client(Some("203.0.113.5"), &[]),
            Some("203.0.113.5".parse().unwrap())
        );
    }

    #[test]
    fn trusted_proxy_is_skipped() {
        // Client 203.0.113.5, proxy 10.0.0.1 (trusted)
        assert_eq!(
            xff_client(Some("203.0.113.5, 10.0.0.1"), &["10.0.0.1".to_string()]),
            Some("203.0.113.5".parse().unwrap())
        );
    }

    #[test]
    fn multiple_trusted_proxies_skipped() {
        // Client 203.0.113.5, P1 10.0.0.1, P2 10.0.0.2 (both trusted)
        assert_eq!(
            xff_client(
                Some("203.0.113.5, 10.0.0.1, 10.0.0.2"),
                &["10.0.0.1".to_string(), "10.0.0.2".to_string()],
            ),
            Some("203.0.113.5".parse().unwrap())
        );
    }

    #[test]
    fn all_trusted_returns_none() {
        assert_eq!(
            xff_client(
                Some("10.0.0.1, 10.0.0.2"),
                &["10.0.0.1".to_string(), "10.0.0.2".to_string()]
            ),
            None
        );
    }

    #[test]
    fn untrusted_hop_returns_first_untrusted_from_right() {
        // Client 203.0.113.5, proxy 10.0.0.1 (trusted), rogue 192.168.1.1 (untrusted)
        assert_eq!(
            xff_client(
                Some("203.0.113.5, 10.0.0.1, 192.168.1.1"),
                &["10.0.0.1".to_string()]
            ),
            Some("192.168.1.1".parse().unwrap())
        );
    }

    #[test]
    fn malformed_hop_terminates_trust_walk() {
        // X-Forwarded-For: 198.51.100.77, malformed, 10.0.0.10
        // Trusted proxy range: 10.0.0.0/8
        // A malformed hop cannot be skipped and the chain must terminate;
        // the caller falls back to the direct peer (10.0.0.10).
        assert_eq!(
            xff_client(
                Some("198.51.100.77, malformed, 10.0.0.10"),
                &["10.0.0.10/8".to_string()],
            ),
            None,
            // fall back to direct peer
        );
    }

    #[test]
    fn ipv6_cidr_trusted_proxy_skipped() {
        // Client 2001:db8::1, proxy 2001:db8::100/120 trusted
        assert_eq!(
            xff_client(
                Some("2001:db8::1, 2001:db8::100"),
                &["2001:db8::100/120".to_string()],
            ),
            Some("2001:db8::1".parse().unwrap())
        );
    }
}
