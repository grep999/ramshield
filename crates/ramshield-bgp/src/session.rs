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
        }
    }

    /// Connect to peer via TCP/179
    pub async fn connect(&mut self) -> Result<()> {
        let stream = TcpStream::connect(self.peer_addr)
            .await
            .map_err(|e| BgpError::Io(e))?;
        stream.set_nodelay(true).map_err(|e| BgpError::Io(e))?;
        self.stream = Some(stream);
        self.connected = true;
        Ok(())
    }

    /// Send OPEN message
    pub async fn send_open(&mut self, version: u8, hold_time: u16) -> Result<()> {
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
        // Minimal UPDATE: header + NLRI path attributes
        // In full impl: withdrawn routes length + path attributes + NLRI
        let encoded = nlri.encode()?;
        let total_len = 19 + encoded.len();
        let mut buf = Vec::with_capacity(total_len);

        buf.extend_from_slice(&[0xFFu8; 16]); // Marker
        buf.extend_from_slice(&(total_len as u16).to_be_bytes()); // Length
        buf.push(2); // UPDATE type

        // Withdrawn routes length (0)
        buf.extend_from_slice(&0u16.to_be_bytes());

        // Total path attribute length
        buf.extend_from_slice(&(encoded.len() as u16).to_be_bytes());

        // Path attributes (FlowSpec NLRI as MP_REACH_NLRI)
        buf.extend_from_slice(&encoded);

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

    /// Read a BGP message from the stream
    pub async fn read_message(&mut self) -> Result<crate::messages::BgpMessage> {
        // Read header first (19 octets)
        let mut header_buf = [0u8; 19];
        let stream = self
            .stream
            .as_mut()
            .ok_or(BgpError::Session("not connected".into()))?;
        stream
            .read_exact(&mut header_buf)
            .await
            .map_err(|e| BgpError::Io(e))?;

        let hdr = crate::messages::BgpHeader::verify(&header_buf)?;

        // Read remaining body if any
        let body_len = hdr.length as usize - 19;
        if body_len > 0 {
            let mut body = vec![0u8; body_len];
            stream
                .read_exact(&mut body)
                .await
                .map_err(|e| BgpError::Io(e))?;
            crate::messages::BgpMessage::from_parts(hdr, Some(body))
        } else {
            crate::messages::BgpMessage::from_parts(hdr, None)
        }
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
        let stream = self
            .stream
            .as_mut()
            .ok_or(BgpError::Session("not connected".into()))?;
        stream.write_all(buf).await.map_err(|e| BgpError::Io(e))?;
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
