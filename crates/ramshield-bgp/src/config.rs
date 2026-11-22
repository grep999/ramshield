use crate::{BgpError, Result};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tokio::time;

/// BGP configuration — runtime knobs for the minimal speaker
#[derive(Debug, Clone)]
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
        }
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
            connect_retry_time: Option<u16>,
        }

        let toml: TomlConfig =
            toml::from_str(s).map_err(|e| BgpError::Session(format!("toml parse error: {e}")))?;

        let peer_ip: IpAddr = toml.peer_ip.parse().map_err(|e| BgpError::AddrParse(e))?;

        let router_id = toml.router_id.unwrap_or(0);
        let mut cfg = Self::new(toml.local_asn, toml.peer_asn, peer_ip);
        if let Some(p) = toml.peer_port {
            cfg.peer_port = p;
        }
        cfg.router_id = router_id;
        if let Some(h) = toml.hold_time {
            cfg.hold_time = h;
        }
        if let Some(c) = toml.connect_retry_time {
            cfg.connect_retry_time = c;
        }
        cfg.keepalive_time = cfg.hold_time / 3;
        Ok(cfg)
    }
}

impl Default for BgpConfig {
    fn default() -> Self {
        Self::new(65001, 174, IpAddr::from([198, 51, 100, 1]))
    }
}
