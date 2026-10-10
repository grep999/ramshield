use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XdpConfig {
    /// Attach the XDP kernel program. When false, enforcement is in-band only.
    #[serde(default)]
    pub enabled: bool,
    /// Interface to attach to (e.g. "eth0", "lo").
    #[serde(default = "default_xdp_iface")]
    pub interface: String,
    /// "skb" (generic, works everywhere) or "drv" (native, production NICs).
    #[serde(default = "default_xdp_mode")]
    pub mode: String,
    /// When XDP attach fails, continue with in-band enforcement (DEGRADED).
    /// Default false: configured XDP that is not attached is FAILED /healthz 503.
    #[serde(default)]
    pub allow_inband_fallback: bool,
    /// Autonomous protection is qualified only for native/driver XDP.
    #[serde(default = "default_require_native_xdp")]
    pub require_native_for_autonomous: bool,
    /// Refuse autonomous startup if RSS cannot be rebalanced.
    #[serde(default)]
    pub require_rss_rebalance: bool,
    /// Trusted cloud/LB source prefixes that must not be interpreted as tenant bans.
    #[serde(default)]
    pub trusted_overlay_cidrs: Vec<ramshield_types::IpNetwork>,
}

impl Default for XdpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: default_xdp_iface(),
            mode: default_xdp_mode(),
            allow_inband_fallback: false,
            require_native_for_autonomous: default_require_native_xdp(),
            require_rss_rebalance: false,
            trusted_overlay_cidrs: Vec::new(),
        }
    }
}

fn default_xdp_iface() -> String {
    "eth0".into()
}

fn default_xdp_mode() -> String {
    "skb".into()
}

fn default_require_native_xdp() -> bool {
    true
}
