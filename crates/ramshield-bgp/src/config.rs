use crate::{BgpError, Result};
use std::net::{IpAddr, Ipv4Addr};

/// BGP configuration — runtime knobs for the standalone speaker
#[derive(Clone)]
pub struct BgpConfig {
    /// Local AS number
    pub local_asn: u16,
    /// Peer AS number
    pub peer_asn: u16,
    /// Peer IP address
    pub peer_ip: IpAddr,
    /// Peer port (179 by default)
    pub peer_port: u16,
    /// Local router ID
    pub router_id: u32,
    /// Hold time in seconds (default 90)
    pub hold_time: u16,
    /// Keepalive interval in seconds (default hold/3)
    pub keepalive_time: u16,
    /// Connect retry interval in seconds (default 120)
    pub connect_retry_time: u16,
    /// Maximum reconnect attempts (default 10)
    pub max_reconnect_attempts: u8,
    /// Schmitt trigger engagement threshold (seconds)
    pub schmitt_engage_secs: u16,
    /// Schmitt trigger disengage threshold (seconds)
    pub schmitt_disengage_secs: u16,
    /// Optional shared key for TCP-MD5/HMAC authentication
    pub peer_shared_key: Option<Vec<u8>>,
}

/// Secret bytes stay out of logs and panic messages.
impl std::fmt::Debug for BgpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BgpConfig")
            .field("local_asn", &self.local_asn)
            .field("peer_asn", &self.peer_asn)
            .field("peer_ip", &self.peer_ip)
            .field("peer_port", &self.peer_port)
            .field("router_id", &self.router_id)
            .field("hold_time", &self.hold_time)
            .field("keepalive_time", &self.keepalive_time)
            .field("connect_retry_time", &self.connect_retry_time)
            .field("max_reconnect_attempts", &self.max_reconnect_attempts)
            .field("schmitt_engage_secs", &self.schmitt_engage_secs)
            .field("schmitt_disengage_secs", &self.schmitt_disengage_secs)
            .field(
                "peer_shared_key",
                &self.peer_shared_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl BgpConfig {
    pub fn new(local_asn: u16, peer_asn: u16, peer_ip: IpAddr) -> Self {
        let hold_time = 90;
        let keepalive_time = hold_time / 3;
        BgpConfig {
            local_asn,
            peer_asn,
            peer_ip,
            peer_port: 179,
            router_id: 0,
            hold_time,
            keepalive_time,
            connect_retry_time: 120,
            max_reconnect_attempts: 10,
            schmitt_engage_secs: 5,
            schmitt_disengage_secs: 15,
            peer_shared_key: None,
        }
    }

    /// Validate runtime invariants (RFC 4271 hold/keepalive, RFC 6793 ASN 0).
    ///
    /// `local_asn`/`peer_asn` are `u16`, so the RFC 6793 range check reduces
    /// to excluding the reserved value 0.
    pub fn validate(&self) -> Result<()> {
        if self.local_asn == 0 || self.peer_asn == 0 {
            return Err(BgpError::Session("ASN 0 is reserved (RFC 6793)".into()));
        }
        if self.peer_port == 0 {
            return Err(BgpError::Session("peer_port must be non-zero".into()));
        }
        // RFC 4271 §4.2: hold time of 1 or 2 seconds is unacceptable.
        if self.hold_time == 1 || self.hold_time == 2 {
            return Err(BgpError::Session(
                "hold_time 1 or 2 is unacceptable (RFC 4271)".into(),
            ));
        }
        if self.hold_time > 0 && self.keepalive_time > self.hold_time {
            return Err(BgpError::Session("keepalive_time exceeds hold_time".into()));
        }
        // RFC 4271 §4.2/§4.4: hold time 0 disables periodic KEEPALIVEs.
        if self.hold_time == 0 && self.keepalive_time > 0 {
            return Err(BgpError::Session(
                "keepalive_time must be 0 when hold_time is 0 (RFC 4271)".into(),
            ));
        }
        if self.connect_retry_time == 0 {
            return Err(BgpError::Session(
                "connect_retry_time must be non-zero".into(),
            ));
        }
        if self.schmitt_engage_secs == 0 {
            return Err(BgpError::Session(
                "schmitt_engage_secs must be non-zero".into(),
            ));
        }
        if self.schmitt_disengage_secs <= self.schmitt_engage_secs {
            return Err(BgpError::Session(
                "schmitt_disengage_secs must exceed schmitt_engage_secs".into(),
            ));
        }
        // RFC 4271 §4.2: the BGP Identifier is the identity of this local
        // speaker, not a value that can be synthesized from its peer or ASN.
        // It must be explicitly configured for both eBGP and iBGP.
        let router_id = Ipv4Addr::from(self.router_id);
        if router_id.is_unspecified() || router_id.is_multicast() || router_id.is_broadcast() {
            return Err(BgpError::Session(
                "BGP requires an explicit locally assigned unicast IPv4 router_id (RFC 4271 §4.2)"
                    .into(),
            ));
        }
        if self.peer_shared_key.as_ref().is_some_and(|k| k.is_empty()) {
            return Err(BgpError::Session(
                "peer_shared_key must not be empty".into(),
            ));
        }
        Ok(())
    }

    /// Load from TOML string (if toml crate available)
    pub fn from_toml(s: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct TomlConfig {
            local_asn: u16,
            peer_asn: u16,
            peer_ip: String,
            peer_port: Option<u16>,
            router_id: Option<u32>,
            hold_time: Option<u16>,
            keepalive_time: Option<u16>,
            connect_retry_time: Option<u16>,
            max_reconnect_attempts: Option<u8>,
            schmitt_engage_secs: Option<u16>,
            schmitt_disengage_secs: Option<u16>,
            peer_shared_key: Option<String>,
        }

        let toml: TomlConfig =
            toml::from_str(s).map_err(|e| BgpError::Session(format!("toml parse error: {e}")))?;

        let peer_ip: IpAddr = toml.peer_ip.parse().map_err(BgpError::AddrParse)?;

        let mut cfg = Self::new(toml.local_asn, toml.peer_asn, peer_ip);
        if let Some(p) = toml.peer_port {
            cfg.peer_port = p;
        }
        cfg.router_id = toml.router_id.unwrap_or(0);
        if let Some(h) = toml.hold_time {
            cfg.hold_time = h;
        }
        // Explicit keepalive wins; otherwise derive from the hold time.
        cfg.keepalive_time = toml.keepalive_time.unwrap_or(cfg.hold_time / 3);
        if let Some(c) = toml.connect_retry_time {
            cfg.connect_retry_time = c;
        }
        if let Some(m) = toml.max_reconnect_attempts {
            cfg.max_reconnect_attempts = m;
        }
        if let Some(e) = toml.schmitt_engage_secs {
            cfg.schmitt_engage_secs = e;
        }
        if let Some(d) = toml.schmitt_disengage_secs {
            cfg.schmitt_disengage_secs = d;
        }
        if let Some(k) = toml.peer_shared_key {
            cfg.peer_shared_key = Some(
                hex::decode(k.trim())
                    .map_err(|e| BgpError::Session(format!("bad peer_shared_key hex: {e}")))?,
            );
        }
        cfg.validate()?;
        Ok(cfg)
    }
}

impl Default for BgpConfig {
    fn default() -> Self {
        Self::new(65001, 174, IpAddr::from([198, 51, 100, 1]))
    }
}

#[cfg(test)]
mod tests {
    use super::BgpConfig;

    #[test]
    fn validate_rejects_invalid_configs() {
        let base = BgpConfig::default();
        let cases: Vec<(&str, BgpConfig)> = vec![
            (
                "zero local ASN",
                BgpConfig {
                    local_asn: 0,
                    ..base.clone()
                },
            ),
            (
                "zero peer ASN",
                BgpConfig {
                    peer_asn: 0,
                    ..base.clone()
                },
            ),
            (
                "port 0",
                BgpConfig {
                    peer_port: 0,
                    ..base.clone()
                },
            ),
            (
                "hold_time 1",
                BgpConfig {
                    hold_time: 1,
                    ..base.clone()
                },
            ),
            (
                "keepalive over hold",
                BgpConfig {
                    hold_time: 30,
                    keepalive_time: 31,
                    ..base.clone()
                },
            ),
            (
                "zero hold with keepalive",
                BgpConfig {
                    hold_time: 0,
                    keepalive_time: 10,
                    ..base.clone()
                },
            ),
            (
                "zero connect_retry_time",
                BgpConfig {
                    connect_retry_time: 0,
                    ..base.clone()
                },
            ),
            (
                "zero engage",
                BgpConfig {
                    schmitt_engage_secs: 0,
                    ..base.clone()
                },
            ),
            (
                "inverted Schmitt thresholds",
                BgpConfig {
                    schmitt_engage_secs: 20,
                    schmitt_disengage_secs: 10,
                    ..base.clone()
                },
            ),
            (
                "iBGP zero router-id",
                BgpConfig {
                    local_asn: 65001,
                    peer_asn: 65001,
                    router_id: 0,
                    ..base.clone()
                },
            ),
            (
                "eBGP zero router-id",
                BgpConfig {
                    local_asn: 65001,
                    peer_asn: 174,
                    router_id: 0,
                    ..base.clone()
                },
            ),
            (
                "multicast router-id",
                BgpConfig {
                    router_id: 0xE000_0001,
                    ..base.clone()
                },
            ),
            (
                "limited broadcast router-id",
                BgpConfig {
                    router_id: u32::MAX,
                    ..base.clone()
                },
            ),
            (
                "empty key",
                BgpConfig {
                    peer_shared_key: Some(Vec::new()),
                    ..base
                },
            ),
        ];
        for (name, cfg) in cases {
            assert!(cfg.validate().is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn validate_requires_explicit_local_router_id_for_ebgp_and_ibgp() {
        let defaults = BgpConfig::default();
        assert!(defaults.validate().is_err());

        let configured = BgpConfig {
            router_id: 0xC000_0201,
            ..defaults
        };
        assert!(configured.validate().is_ok());

        let ibgp = BgpConfig {
            local_asn: 65001,
            peer_asn: 65001,
            router_id: 0xC000_0201,
            ..configured
        };
        assert!(ibgp.validate().is_ok());
    }

    #[test]
    fn from_toml_parses_optional_runtime_knobs() {
        let cfg = BgpConfig::from_toml(
            "local_asn = 65001\npeer_asn = 65002\npeer_ip = \"10.0.0.1\"\nrouter_id = 3221225985\n\
             hold_time = 90\nkeepalive_time = 20\nmax_reconnect_attempts = 3\n\
             schmitt_engage_secs = 7\nschmitt_disengage_secs = 21\n",
        )
        .unwrap();
        assert_eq!(cfg.keepalive_time, 20);
        assert_eq!(cfg.max_reconnect_attempts, 3);
        assert_eq!(cfg.schmitt_engage_secs, 7);
        assert_eq!(cfg.schmitt_disengage_secs, 21);
    }

    #[test]
    fn from_toml_derives_keepalive_and_validates() {
        let cfg = BgpConfig::from_toml(
            "local_asn = 65001\npeer_asn = 65002\npeer_ip = \"10.0.0.1\"\nrouter_id = 3221225985\nhold_time = 60\n",
        )
        .unwrap();
        assert_eq!(cfg.keepalive_time, 20);

        assert!(
            BgpConfig::from_toml(
                "local_asn = 65001\npeer_asn = 65002\npeer_ip = \"10.0.0.1\"\nrouter_id = 3221225985\nhold_time = 1\n",
            )
            .is_err()
        );
    }
}
