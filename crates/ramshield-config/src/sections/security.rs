use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousConfig {
    /// Enable first-line packet protection in XDP without waiting for userspace.
    #[serde(default)]
    pub enabled: bool,
    /// Per-CPU SYN packets/sec budget. 0 disables the SYN guard.
    #[serde(default = "default_autonomous_syn_pps")]
    pub syn_pps_per_cpu: u64,
    /// Per-CPU UDP packets/sec budget. 0 disables the UDP guard.
    #[serde(default = "default_autonomous_udp_pps")]
    pub udp_pps_per_cpu: u64,
    /// Per-CPU packets/sec budget for all parsed IP traffic. 0 disables it.
    #[serde(default = "default_autonomous_packet_pps")]
    pub packet_pps_per_cpu: u64,
    /// Fixed kernel accounting window. Keep small for cold-start protection.
    #[serde(default = "default_autonomous_window_ms")]
    pub window_ms: u64,
}
impl Default for AutonomousConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            syn_pps_per_cpu: default_autonomous_syn_pps(),
            udp_pps_per_cpu: default_autonomous_udp_pps(),
            packet_pps_per_cpu: default_autonomous_packet_pps(),
            window_ms: default_autonomous_window_ms(),
        }
    }
}
fn default_autonomous_syn_pps() -> u64 {
    25_000
}
fn default_autonomous_udp_pps() -> u64 {
    50_000
}
fn default_autonomous_packet_pps() -> u64 {
    200_000
}
fn default_autonomous_window_ms() -> u64 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeIngestConfig {
    /// Observe L3/L4 traffic directly from the host NIC on Linux.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_native_ingest_interface")]
    pub interface: String,
    /// Maximum telemetry events emitted per second. SYN packets are always
    /// admitted until this cap is reached; ordinary packets are sampled.
    #[serde(default = "default_native_ingest_max_events_per_sec")]
    pub max_events_per_sec: u64,
}
impl Default for NativeIngestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: default_native_ingest_interface(),
            max_events_per_sec: default_native_ingest_max_events_per_sec(),
        }
    }
}
fn default_native_ingest_interface() -> String {
    "eth0".into()
}
fn default_native_ingest_max_events_per_sec() -> u64 {
    20_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynproxyConfig {
    /// Enable Linux kernel SYNPROXY for configured TCP ports. This is a
    /// kernel-stateful defense complementary to the XDP stateless guard.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_synproxy_ports")]
    pub ports: Vec<u16>,
    #[serde(default = "default_synproxy_mss")]
    pub mss: u16,
    #[serde(default = "default_synproxy_wscale")]
    pub wscale: u8,
    /// Maximum tracked TCP connections to the protected ports from one source IP.
    #[serde(default = "default_synproxy_max_connections_per_source")]
    pub max_connections_per_source: u32,
    /// Maximum total tracked TCP connections to the protected ports.
    #[serde(default = "default_synproxy_max_connections_total")]
    pub max_connections_total: u32,
    /// New tracked TCP connections per source IP per second.
    #[serde(default = "default_synproxy_new_connections_per_second")]
    pub new_connections_per_second: u32,
    /// Initial burst permitted above the steady connection rate.
    #[serde(default = "default_synproxy_new_connection_burst")]
    pub new_connection_burst: u32,
    /// Global new TCP connection rate across protected ports.
    #[serde(default = "default_synproxy_new_connections_total_per_second")]
    pub new_connections_total_per_second: u32,
    /// Global initial connection burst.
    #[serde(default = "default_synproxy_new_connection_total_burst")]
    pub new_connection_total_burst: u32,
}
impl Default for SynproxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ports: default_synproxy_ports(),
            mss: default_synproxy_mss(),
            wscale: default_synproxy_wscale(),
            max_connections_per_source: default_synproxy_max_connections_per_source(),
            max_connections_total: default_synproxy_max_connections_total(),
            new_connections_per_second: default_synproxy_new_connections_per_second(),
            new_connection_burst: default_synproxy_new_connection_burst(),
            new_connections_total_per_second: default_synproxy_new_connections_total_per_second(),
            new_connection_total_burst: default_synproxy_new_connection_total_burst(),
        }
    }
}
fn default_synproxy_ports() -> Vec<u16> {
    vec![22, 53, 80, 443]
}
fn default_synproxy_mss() -> u16 {
    1460
}
fn default_synproxy_wscale() -> u8 {
    7
}
fn default_synproxy_max_connections_per_source() -> u32 {
    128
}
fn default_synproxy_max_connections_total() -> u32 {
    16384
}
fn default_synproxy_new_connections_per_second() -> u32 {
    50
}
fn default_synproxy_new_connection_burst() -> u32 {
    100
}
fn default_synproxy_new_connections_total_per_second() -> u32 {
    2000
}
fn default_synproxy_new_connection_total_burst() -> u32 {
    4000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mesh_node_id")]
    pub node_id: u32,
    #[serde(default = "default_mesh_listen_addr")]
    pub listen_addr: String,
    #[serde(default)]
    pub peers: Vec<String>,
    /// Hex-encoded shared secret. Empty disables authenticated mesh transport.
    #[serde(default)]
    pub auth_key: String,
}
impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: default_mesh_node_id(),
            listen_addr: default_mesh_listen_addr(),
            peers: Vec::new(),
            auth_key: String::new(),
        }
    }
}
fn default_mesh_node_id() -> u32 {
    1
}
fn default_mesh_listen_addr() -> String {
    "127.0.0.1:7900".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_upstream_interface")]
    pub interface: String,
    /// Link capacity in Mbps. Zero disables saturation calculations.
    #[serde(default)]
    pub link_capacity_mbps: u64,
    #[serde(default = "default_upstream_saturation_pct")]
    pub saturation_pct: u8,
    #[serde(default = "default_upstream_poll_ms")]
    pub poll_ms: u64,
    /// Optional plain-HTTP webhook. Put TLS in front of it with the existing deployment proxy.
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default = "default_upstream_cooldown_secs")]
    pub cooldown_secs: u64,
    #[serde(default)]
    pub bgp_mode: String,
    #[serde(default)]
    pub bgp_fifo: Option<String>,
    #[serde(default)]
    pub protected_prefix: Option<String>,
    #[serde(default)]
    pub bgp_community: Option<String>,
}
impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: default_upstream_interface(),
            link_capacity_mbps: 0,
            saturation_pct: default_upstream_saturation_pct(),
            poll_ms: default_upstream_poll_ms(),
            webhook_url: None,
            cooldown_secs: default_upstream_cooldown_secs(),
            bgp_mode: "none".into(),
            bgp_fifo: None,
            protected_prefix: None,
            bgp_community: None,
        }
    }
}
fn default_upstream_interface() -> String {
    "eth0".into()
}
fn default_upstream_saturation_pct() -> u8 {
    90
}
fn default_upstream_poll_ms() -> u64 {
    1000
}
fn default_upstream_cooldown_secs() -> u64 {
    60
}
