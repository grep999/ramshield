use serde::{Deserialize, Serialize};

/// Protocol version for compatibility checks.
pub const PROTOCOL_VERSION: u16 = 1;

/// Top-level message envelope with version field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub version: u16,
    pub body: Body,
}

impl Message {
    pub fn new(body: Body) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            body,
        }
    }

    pub fn request(req: Request) -> Self {
        Self::new(Body::Request(req))
    }

    pub fn response(resp: Response) -> Self {
        Self::new(Body::Response(resp))
    }
}

/// Message body: either a request or response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Body {
    Request(Request),
    Response(Response),
}

/// Client-to-server requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    ReportConnection {
        ip: String,
        bytes: u64,
        status_code: u16,
        proto_fp: u32,
    },
    ReportConnections {
        events: Vec<ConnectionReport>,
    },
    Flush,
}

/// Server-to-client responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Connection report for batch ingestion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionReport {
    pub ip: String,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fp: u32,
}

/// Global statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stats {
    pub ips_tracked: usize,
    pub blocked: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub uptime_secs: u64,
    pub evictions: u64,
}

/// Per-IP detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_version_roundtrip() {
        let msg = Message::request(Request::GetStatus);
        assert_eq!(msg.version, PROTOCOL_VERSION);
    }
}
