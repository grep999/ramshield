use crate::attributes::{MpReachNlri, PathAttr, PathAttrFlags, PathAttrType};
use crate::flowspec::FlowSpecNLRI;
use crate::messages::BgpKeepalive;
use crate::{BgpError, Result};
use std::net::{IpAddr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// BGP TCP session management (TCP/179)
pub struct BgpSession {
    /// TCP stream to peer
    stream: Option<TcpStream>,
    /// Peer address
    peer_addr: SocketAddr,
    /// Local ASN
    local_asn: u16,
    /// Peer ASN
    peer_asn: u16,
    /// Router ID
    router_id: u32,
    /// Connection state
    connected: bool,
    /// Negotiated hold time (seconds), set by send_open
    hold_time: u16,
    /// Optional HMAC key for peer authentication
    hmac_key: Option<Vec<u8>>,
}

impl BgpSession {
    pub fn new(local_asn: u16, peer_asn: u16, peer_ip: IpAddr, peer_port: u16) -> Self {
        let peer_addr = SocketAddr::new(peer_ip, peer_port);
        BgpSession {
            stream: None,
            peer_addr,
            local_asn,
            peer_asn,
            router_id: 0,
            connected: false,
            hold_time: 0,
            hmac_key: None,
        }
    }

    /// Build a session from config, carrying the optional HMAC key.
    pub fn from_config(config: &crate::config::BgpConfig) -> Self {
        let mut session = Self::new(
            config.local_asn,
            config.peer_asn,
            config.peer_ip,
            config.peer_port,
        );
        if let Some(key) = &config.peer_shared_key {
            session.set_hmac_key(key.clone());
        }
        session
    }

    /// Connect to peer via TCP/179
    pub async fn connect(&mut self) -> Result<()> {
        let stream = TcpStream::connect(self.peer_addr)
            .await
            .map_err(BgpError::Io)?;
        stream.set_nodelay(true).map_err(BgpError::Io)?;
        self.stream = Some(stream);
        self.connected = true;
        Ok(())
    }

    /// Send OPEN message
    pub async fn send_open(&mut self, version: u8, hold_time: u16) -> Result<()> {
        self.hold_time = hold_time;
        // Build OPEN message: marker(16) + length(19+opt_params) + type(1)
        // + version(1) + my_asn(2) + hold_time(2) + bgp_id(4) + opt_params_len(1) + opt_params(var)
        let opt_params_len = 0; // Simplified: no optional params in minimal impl
        let msg_len = 19 + opt_params_len;
        let mut buf = Vec::with_capacity(msg_len);

        buf.extend_from_slice(&[0xFFu8; 16]); // Marker
        buf.extend_from_slice(&msg_len.to_be_bytes()); // Length
        buf.push(1); // OPEN type

        buf.push(version); // BGP version
        buf.extend_from_slice(&self.local_asn.to_be_bytes()); // My ASN
        buf.extend_from_slice(&hold_time.to_be_bytes()); // Hold time
        buf.extend_from_slice(&self.router_id.to_be_bytes()); // BGP identifier
        buf.push(opt_params_len as u8); // Opt params length

        self.send_raw(&buf).await
    }

    /// Send UPDATE message (FlowSpec or RTBH)
    pub async fn send_update(&mut self, nlri: FlowSpecNLRI) -> Result<()> {
        let encoded = nlri.encode()?;
        let mp_reach = MpReachNlri {
            afi: 1,    // IPv4
            safi: 133, // FlowSpec
            next_hop: vec![0, 0, 0, 0],
            nlri: encoded,
        }
        .encode();
        let attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: true,
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::MpReachNlri,
            value: mp_reach,
        }
        .encode();
        // marker(16) + length(2) + type(1) + withdrawn_len(2) + attr_len(2) + attrs
        let total_len = 23usize + attr.len();
        let length = u16::try_from(total_len)
            .map_err(|_| BgpError::Session(format!("UPDATE too large: {total_len}")))?;
        if length > 4096 {
            return Err(BgpError::Session(format!("UPDATE too large: {length}")));
        }
        let mut buf = Vec::with_capacity(total_len);
        buf.extend_from_slice(&[0xFFu8; 16]);
        buf.extend_from_slice(&length.to_be_bytes());
        buf.push(2); // UPDATE
        buf.extend_from_slice(&0u16.to_be_bytes()); // withdrawn routes
        buf.extend_from_slice(&(attr.len() as u16).to_be_bytes());
        buf.extend_from_slice(&attr);
        self.send_raw(&buf).await
    }

    /// Send KEEPALIVE message
    pub async fn send_keepalive(&mut self) -> Result<()> {
        let data = BgpKeepalive::encode();
        self.send_raw(&data).await
    }

    /// Send NOTIFICATION message
    pub async fn send_notification(&mut self, error_code: u8, error_subcode: u8) -> Result<()> {
        let mut buf = Vec::with_capacity(21);
        buf.extend_from_slice(&[0xFFu8; 16]);
        buf.extend_from_slice(&21u16.to_be_bytes());
        buf.push(3); // NOTIFICATION type
        buf.push(error_code);
        buf.push(error_subcode);
        self.send_raw(&buf).await
    }

    /// Configure an HMAC-SHA256 key for peer authentication.
    /// When set, every received message must carry a 32-byte tag
    /// appended after the message; the tag is verified in constant time.
    pub fn set_hmac_key(&mut self, key: Vec<u8>) {
        self.hmac_key = Some(key);
    }

    /// Read a BGP message from the stream
    pub async fn read_message(&mut self) -> Result<crate::messages::BgpMessage> {
        // Hold timer is 0 until OPEN is sent; use a conservative 90s default
        // so a stalled peer cannot block the reader indefinitely.
        let timeout_dur = tokio::time::Duration::from_secs(if self.hold_time == 0 {
            90
        } else {
            self.hold_time.max(1)
        } as u64);
        let mut raw_buf = Vec::with_capacity(4096);
        let mut header_buf = [0u8; 19];
        let stream = self
            .stream
            .as_mut()
            .ok_or(BgpError::Session("not connected".into()))?;
        tokio::time::timeout(timeout_dur, stream.read_exact(&mut header_buf))
            .await
            .map_err(|_| BgpError::Session("read timeout".into()))?
            .map_err(BgpError::Io)?;
        raw_buf.extend_from_slice(&header_buf);

        let hdr = crate::messages::BgpHeader::verify(&header_buf)?;

        let body_len = hdr.length as usize - 19;
        if body_len > 0 {
            let mut body = vec![0u8; body_len];
            tokio::time::timeout(timeout_dur, stream.read_exact(&mut body))
                .await
                .map_err(|_| BgpError::Session("read timeout".into()))?
                .map_err(BgpError::Io)?;
            raw_buf.extend_from_slice(&body);
        }

        if let Some(key) = self.hmac_key.as_ref() {
            let mut tag = [0u8; 32];
            tokio::time::timeout(timeout_dur, stream.read_exact(&mut tag))
                .await
                .map_err(|_| BgpError::Session("read timeout".into()))?
                .map_err(BgpError::Io)?;
            crate::verify_hmac(&raw_buf, key, &tag)?;
        }

        crate::messages::BgpMessage::from_parts(
            hdr,
            if body_len > 0 {
                Some(raw_buf[19..].to_vec())
            } else {
                None
            },
        )
    }

    /// Validate a received OPEN message against configured peer identity.
    /// Sends NOTIFICATION and returns Err on mismatch (RFC 4271 §6.2).
    pub async fn validate_open(&mut self, open: &crate::messages::BgpOpen) -> Result<()> {
        if open.version != 4 {
            let _ = self.send_notification(2, 1).await; // Unsupported Version
            return Err(BgpError::Session("unsupported BGP version".into()));
        }
        if open.my_asn != self.peer_asn {
            let _ = self.send_notification(2, 2).await; // Bad Peer AS
            return Err(BgpError::Session("peer ASN mismatch".into()));
        }
        if open.hold_time != 0 && open.hold_time < 3 {
            let _ = self.send_notification(2, 6).await; // Unacceptable Hold Time
            return Err(BgpError::Session("unacceptable hold time".into()));
        }
        Ok(())
    }

    /// Close connection
    pub async fn close(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
        self.connected = false;
    }

    /// Is connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Send raw bytes to the peer
    async fn send_raw(&mut self, buf: &[u8]) -> Result<()> {
        let hmac_tag = if let Some(key) = self.hmac_key.as_ref() {
            let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key)
                .map_err(|_| BgpError::Session("invalid HMAC key".into()))?;
            use hmac::Mac;
            mac.update(buf);
            Some(mac.finalize().into_bytes().to_vec())
        } else {
            None
        };
        let stream = self
            .stream
            .as_mut()
            .ok_or(BgpError::Session("not connected".into()))?;
        stream.write_all(buf).await.map_err(BgpError::Io)?;
        if let Some(tag) = hmac_tag {
            stream.write_all(&tag).await.map_err(BgpError::Io)?;
        }
        Ok(())
    }

    /// Retry with exponential backoff: 1s, 2s, 4s... max 30s, up to 10 attempts
    pub async fn retry_connect(&mut self) -> Result<()> {
        let mut delay = tokio::time::Duration::from_secs(1);
        let max_delay = tokio::time::Duration::from_secs(30);
        let mut attempts = 0;

        while attempts < 10 {
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(_) => {
                    attempts += 1;
                    if attempts >= 10 {
                        return Err(BgpError::Session("max reconnect attempts reached".into()));
                    }
                    tokio::time::sleep(delay).await;
                    if delay < max_delay {
                        delay *= 2;
                    }
                }
            }
        }
        Err(BgpError::Session("reconnect failed".into()))
    }
}

impl Default for BgpSession {
    fn default() -> Self {
        BgpSession::new(65001, 174, IpAddr::from([198, 51, 100, 1]), 179)
    }
}
