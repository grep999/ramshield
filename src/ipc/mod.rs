use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionReport {
    pub ip: String,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fp: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    CheckIp {
        ip: String,
    },
    BlockIp {
        ip: String,
        reason: String,
        ttl_secs: Option<u64>,
    },
    UnblockIp {
        ip: String,
    },
    GetIpStats {
        ip: String,
    },
    GetStats,
    GetStatus,
    /// Single event — fully compatible with existing integrations.
    ReportConnection {
        ip: String,
        bytes: u64,
        status_code: u16,
        proto_fp: u32,
    },
    /// High-throughput path: many events per IPC round-trip.
    ReportConnections {
        events: Vec<ConnectionReport>,
    },
    Flush,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    IpStatus {
        ip: String,
        blocked: bool,
        threat: f32,
        ewma_rps: f64,
        reason: Option<String>,
    },
    Ok {
        message: String,
    },
    BatchOk {
        accepted: u32,
        rejected: u32,
    },
    Error {
        code: u32,
        message: String,
    },
    Stats(Stats),
    IpDetail(IpDetail),
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub ips_tracked: usize,
    pub blocked: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub uptime_secs: u64,
    pub evictions: u64,
}

#[derive(Debug, Serialize)]
pub struct IpDetail {
    pub ip: String,
    pub count: u64,
    pub ewma_rps: f64,
    pub threat: f32,
    pub state: String,
    pub bytes_in: u64,
    pub first_seen_s: u64,
    pub last_seen_s: u64,
}
