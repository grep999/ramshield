use crate::attributes::{
    MpReachNlri, PathAttr, PathAttrFlags, PathAttrType, as_path_attr, origin_attr,
};
use crate::flowspec::FlowSpecNLRI;
use crate::messages::BgpKeepalive;
use crate::{BgpError, Result};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Peer's BGP identifier, learned from OPEN (RFC 4271 §4.2)
    peer_router_id: u32,
    /// Connection state
    connected: bool,
    /// Negotiated hold time (seconds), set by send_open
    hold_time: u16,
    /// Explicit keepalive override from config; falls back to hold/3
    keepalive_override: Option<u16>,
    /// Reconnect attempt ceiling from config (default 10)
    max_reconnect_attempts: u8,
    /// Deadline for a single socket write (test-overridable)
    write_timeout: Duration,
    /// Optional HMAC key for peer authentication
    hmac_key: Option<Vec<u8>>,
    /// Whether the peer advertised MP-BGP SAFI 133 (RFC 4760 §5).
    peer_flowspec: bool,
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
            peer_router_id: 0,
            connected: false,
            hold_time: 0,
            keepalive_override: None,
            max_reconnect_attempts: 10,
            write_timeout: WRITE_TIMEOUT,
            hmac_key: None,
            peer_flowspec: false,
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
        // RFC 4271 §6.4: a BGP identifier of 0 is invalid. High bits 0xFFFF
        // keep the derived id away from real unicast router-ids (the common
        // case that the peer uses its own address) and the §6.7 collision
        // check. In iBGP an ASN-only derivation collides with the peer's own
        // derived id, so fold in the peer address.
        // Router ID is local-speaker identity and must not vary by peer.
        // iBGP requires an explicitly configured ID (BgpConfig::validate).
        // The eBGP fallback is stable per local ASN for simple deployments;
        // multi-router production deployments should configure it explicitly.
        session.router_id = if config.router_id != 0 {
            config.router_id
        } else if config.local_asn == config.peer_asn {
            0
        } else {
            0x0AFF_0000u32 | u32::from(config.local_asn)
        };
        if config.keepalive_time > 0 {
            session.keepalive_override = Some(config.keepalive_time);
        }
        session.max_reconnect_attempts = config.max_reconnect_attempts;
        // RFC 4271 §6.2: hold time must be known before OPEN is received so
        // validate_open can compute the negotiated minimum.
        session.hold_time = config.hold_time;
        if let Some(key) = &config.peer_shared_key {
            session.set_hmac_key(key.clone());
        }
        session
    }

    /// Connect to peer via TCP/179
    pub async fn connect(&mut self) -> Result<()> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(self.peer_addr))
            .await
            .map_err(|_| BgpError::Session("connect timeout".into()))?
            .map_err(BgpError::Io)?;
        stream.set_nodelay(true).map_err(BgpError::Io)?;
        self.stream = Some(stream);
        self.connected = true;
        Ok(())
    }

    /// Send OPEN message.
    ///
    /// RFC 4271 §4.2: the optional parameters field carries capability
    /// advertisements. A speaker that wants a non-default AFI/SAFI MUST
    /// advertise the corresponding MP-BGP capability (RFC 4760 §5), because
    /// a peer receiving NLRI for an unadvertised SAFI MUST treat the UPDATE
    /// as malformed. Without this, SAFI 133 FlowSpec can never be used.
    pub async fn send_open(&mut self, version: u8, hold_time: u16) -> Result<()> {
        if self.router_id == 0 {
            return Err(BgpError::Session(
                "a non-zero, locally assigned router_id is required before sending OPEN".into(),
            ));
        }
        if hold_time == 1 || hold_time == 2 {
            return Err(BgpError::Session("OPEN hold time 1 or 2 is unacceptable".into()));
        }
        self.hold_time = hold_time;
        // Capability: Multiprotocol Extensions, AFI 1 (IPv4), SAFI 133
        // (FlowSpec). Encoded as a type-2 optional parameter.
        let mut cap = Vec::with_capacity(4 + 2);
        cap.push(1); // capability code: multiprotocol
        cap.push(4); // capability length
        cap.extend_from_slice(&1u16.to_be_bytes()); // AFI IPv4
        cap.push(0); // reserved
        cap.push(133); // SAFI FlowSpec

        let mut opt_params = Vec::with_capacity(cap.len() + 2);
        opt_params.push(2); // parameter type: capability
        opt_params.push(cap.len() as u8);
        opt_params.extend_from_slice(&cap);

        let msg_len = 29usize + opt_params.len();
        let length =
            u16::try_from(msg_len).map_err(|_| BgpError::Session("OPEN too large".into()))?;
        let mut buf = Vec::with_capacity(msg_len);
        buf.extend_from_slice(&[0xFFu8; 16]); // Marker
        buf.extend_from_slice(&length.to_be_bytes()); // Length
        buf.push(1); // OPEN type

        buf.push(version); // BGP version
        buf.extend_from_slice(&self.local_asn.to_be_bytes()); // My ASN
        buf.extend_from_slice(&hold_time.to_be_bytes()); // Hold time
        buf.extend_from_slice(&self.router_id.to_be_bytes()); // BGP identifier
        buf.push(opt_params.len() as u8); // Opt params length
        buf.extend_from_slice(&opt_params);

        self.send_raw(&buf).await
    }

    /// Send UPDATE message (FlowSpec NLRI with an action community).
    ///
    /// Wire layout per RFC 8955 §4.2.2: the FlowSpec NLRI is carried in
    /// MP_REACH_NLRI with SAFI 133. Components are <type, op/value...> as
    /// produced by [`FlowSpecNLRI::encode`].
    ///
    /// `action` MUST be present: a FlowSpec rule with no traffic-filtering
    /// action (RFC 8955 §5) is a no-op on every conforming router.
    ///
    /// The peer MUST have advertised MP-BGP AFI 1 / SAFI 133 in OPEN; call
    /// [`Self::validate_open`] before this method. RFC 4760 §5 requires the
    /// capability check before any MP_REACH_NLRI is sent.
    pub async fn send_update(
        &mut self,
        nlri: FlowSpecNLRI,
        action: crate::attributes::ExtendedCommunity,
    ) -> Result<()> {
        if !self.peer_flowspec {
            return Err(BgpError::Session(
                "peer did not advertise MP-BGP SAFI 133; call validate_open first".into(),
            ));
        }
        let encoded = nlri.encode()?;
        let mp_reach = MpReachNlri {
            afi: 1,               // IPv4
            safi: 133,            // FlowSpec
            next_hop: Vec::new(), // RFC 8955 section 4: length MUST be 0
            nlri: encoded,
        }
        .try_encode()?;
        let mp_reach_attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: false, // RFC 4760 §3: MP_REACH_NLRI is optional non-transitive
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::MpReachNlri,
            value: mp_reach,
        };
        // RFC 8955 §5: the filtering action travels as a transitive
        // extended community (type 0x07 for generic traffic filtering).
        let ext_attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: true,
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::ExtendedCommunities,
            value: action.encode().to_vec(),
        };
        let attrs = crate::attributes::encode_attrs(&[
            origin_attr(),
            as_path_attr(self.local_asn, self.peer_asn),
            mp_reach_attr,
            ext_attr,
        ])?;
        let total_len = 23usize + attrs.len();
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
        buf.extend_from_slice(&(attrs.len() as u16).to_be_bytes());
        buf.extend_from_slice(&attrs);
        self.send_raw(&buf).await
    }

    /// Send KEEPALIVE message
    pub async fn send_keepalive(&mut self) -> Result<()> {
        let data = BgpKeepalive::encode();
        self.send_raw(&data).await
    }

    /// Run the keepalive timer until the stream closes.
    ///
    /// RFC 4271 §4.5: the speaker MUST send a KEEPALIVE every
    /// `keepalive_time` seconds while Established, otherwise the peer's hold
    /// timer expires and the session is torn down.
    ///
    /// ponytail: one interval task per session, no timer wheel. Upgrade path:
    /// a shared timer wheel once a node runs more than a handful of sessions.
    pub async fn run_keepalive(
        &mut self,
        keepalive_time: u16,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        // RFC 4271 §4.2: a negotiated hold time of 0 disables keepalives.
        if keepalive_time == 0 {
            return Ok(());
        }
        let period = tokio::time::Duration::from_secs(keepalive_time.max(1) as u64);
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Consume the immediate first tick so the first write waits a full
        // interval after OPEN/KEEPALIVE negotiation.
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = self.send_keepalive().await {
                        self.connected = false;
                        return Err(e);
                    }
                }
                change = shutdown.changed() => {
                    // Err means every sender was dropped: the task that
                    // owns shutdown is gone, so stop instead of spinning.
                    if change.is_err() || *shutdown.borrow() {
                        self.connected = false;
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Negotiated keepalive interval, derived from the accepted hold time.
    /// RFC 4271 §6.2: zero negotiated hold time means no keepalives.
    pub fn negotiated_keepalive(&self) -> u16 {
        if self.hold_time == 0 {
            return 0;
        }
        let safe_default = (self.hold_time / 3).max(1);
        self.keepalive_override
            .map(|override_secs| override_secs.min(safe_default))
            .unwrap_or(safe_default)
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

        // Header verified in isolation; the full PDU must now be present.
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
        // Reject a truncated PDU: every byte the header announced must be
        // present before HMAC verification (RFC 4271 §6).
        if raw_buf.len() != hdr.length as usize {
            return Err(BgpError::TooShort {
                expected: hdr.length as usize,
                got: raw_buf.len(),
            });
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
        if open.bgp_id == 0 {
            // RFC 4271 §8: OPEN Message Error / Bad BGP Identifier.
            let _ = self.send_notification(2, 3).await;
            return Err(BgpError::Session("peer sent zero BGP identifier".into()));
        }
        if open.hold_time != 0 && open.hold_time < 3 {
            let _ = self.send_notification(2, 6).await; // Unacceptable Hold Time
            return Err(BgpError::Session("unacceptable hold time".into()));
        }
        // RFC 4271 §6.7: collision detection. If both speakers use the same
        // identifier, the session is torn down with a NOTIFICATION.
        if open.bgp_id == self.router_id {
            let _ = self.send_notification(6, 7).await; // Connection Collision
            return Err(BgpError::Session("BGP identifier collision".into()));
        }
        // RFC 4271 §6.2: negotiated hold time is the lower of the two
        // non-zero values; zero on either side disables hold/keepalive.
        self.hold_time = match (self.hold_time, open.hold_time) {
            (0, _) | (_, 0) => 0,
            (local, peer) => local.min(peer),
        };
        // RFC 4760 §5: sending NLRI for an unadvertised SAFI is a malformed
        // UPDATE. Remember whether the peer advertised AFI 1 / SAFI 133.
        self.peer_flowspec = open
            .opt_params
            .iter()
            .any(|cap| cap.code == 1 && cap.length >= 4 && cap.value == [0, 1, 0, 133]);
        self.peer_router_id = open.bgp_id;
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
        let write_timeout = self.write_timeout;
        let stream = self
            .stream
            .as_mut()
            .ok_or(BgpError::Session("not connected".into()))?;
        write_all_with_timeout(stream, buf, write_timeout).await?;
        if let Some(tag) = hmac_tag {
            write_all_with_timeout(stream, &tag, write_timeout).await?;
        }
        Ok(())
    }

    /// Retry with exponential backoff: 1s, 2s, 4s... max 30s, up to the
    /// configured attempt ceiling (default 10).
    pub async fn retry_connect(&mut self) -> Result<()> {
        let max_attempts = self.max_reconnect_attempts.max(1);
        let mut delay = tokio::time::Duration::from_secs(1);
        let max_delay = tokio::time::Duration::from_secs(30);
        let mut attempts = 0;

        while attempts < max_attempts {
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(_) => {
                    attempts += 1;
                    if attempts >= max_attempts {
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

/// Bounded socket write; cancellation on expiry aborts a pending write.
async fn write_all_with_timeout<W: AsyncWrite + Unpin>(
    stream: &mut W,
    buf: &[u8],
    limit: Duration,
) -> Result<()> {
    tokio::time::timeout(limit, stream.write_all(buf))
        .await
        .map_err(|_| BgpError::Session("write timeout".into()))?
        .map_err(BgpError::Io)
}

impl Default for BgpSession {
    fn default() -> Self {
        BgpSession::new(65001, 174, IpAddr::from([198, 51, 100, 1]), 179)
    }
}

#[cfg(test)]
mod tests {
    use super::BgpSession;
    use crate::config::BgpConfig;

    #[test]
    fn from_config_preserves_configured_router_id() {
        let cfg = BgpConfig {
            router_id: 0x0A00_0001,
            ..BgpConfig::default()
        };
        assert_eq!(BgpSession::from_config(&cfg).router_id, 0x0A00_0001);
    }

    #[test]
    fn from_config_derives_router_id_when_unset() {
        let cfg = BgpConfig::default();
        assert_eq!(cfg.router_id, 0);
        let derived = BgpSession::from_config(&cfg).router_id;
        assert_eq!(derived, 0x0AFF_0000 | u32::from(cfg.local_asn));
    }

    #[test]
    fn from_config_carries_keepalive_and_max_attempts() {
        let cfg = BgpConfig {
            keepalive_time: 20,
            max_reconnect_attempts: 3,
            hold_time: 90,
            ..BgpConfig::default()
        };
        let session = BgpSession::from_config(&cfg);
        assert_eq!(session.negotiated_keepalive(), 20);
        assert_eq!(session.max_reconnect_attempts, 3);

        let defaults = BgpSession::default();
        assert_eq!(defaults.max_reconnect_attempts, 10);
        // BgpSession::new starts with hold_time 0 until OPEN is sent.
        assert_eq!(defaults.negotiated_keepalive(), 0);
    }

    #[tokio::test]
    async fn validate_open_negotiates_hold_time() {
        let cfg = BgpConfig {
            hold_time: 90,
            keepalive_time: 0,
            ..BgpConfig::default()
        };
        let mut session = BgpSession::from_config(&cfg);
        let open = crate::messages::BgpOpen {
            version: 4,
            my_asn: cfg.peer_asn,
            hold_time: 30,
            bgp_id: 0x0A00_0002,
            opt_params: Vec::new(),
        };
        session.validate_open(&open).await.unwrap();
        assert_eq!(session.hold_time, 30);
        assert_eq!(session.negotiated_keepalive(), 10);
    }

    #[tokio::test]
    async fn validate_open_zero_hold_time_disables_keepalive() {
        let cfg = BgpConfig {
            hold_time: 90,
            keepalive_time: 0,
            ..BgpConfig::default()
        };
        let mut session = BgpSession::from_config(&cfg);
        let open = crate::messages::BgpOpen {
            version: 4,
            my_asn: cfg.peer_asn,
            hold_time: 0,
            bgp_id: 0x0A00_0002,
            opt_params: Vec::new(),
        };
        session.validate_open(&open).await.unwrap();
        assert_eq!(session.hold_time, 0);
        assert_eq!(session.negotiated_keepalive(), 0);
    }

    #[tokio::test]
    async fn validate_open_remembers_flowspec_capability() {
        let cfg = BgpConfig::default();
        let mut session = BgpSession::from_config(&cfg);
        let open = crate::messages::BgpOpen {
            version: 4,
            my_asn: cfg.peer_asn,
            hold_time: 90,
            bgp_id: 0x0A00_0002,
            opt_params: vec![crate::messages::BgpCapability {
                code: 1,
                length: 4,
                value: [0, 1, 0, 133],
            }],
        };
        session.validate_open(&open).await.unwrap();
        assert!(session.peer_flowspec, "MP-BGP SAFI 133 must be remembered");

        // Without capability, send_update fails before touching the socket.
        let mut no_cap = BgpSession::from_config(&cfg);
        no_cap
            .validate_open(&crate::messages::BgpOpen {
                opt_params: Vec::new(),
                ..open.clone()
            })
            .await
            .unwrap();
        let fs = crate::flowspec::FlowSpecNLRI::rtbh_flowspec("198.51.100.1").unwrap();
        let err = no_cap
            .send_update(fs, crate::attributes::ExtendedCommunity::flowspec_discard())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("SAFI 133"), "error: {err}");
    }

    #[test]
    fn from_config_ibgp_router_id_uses_peer_address() {
        let base = BgpConfig {
            local_asn: 65001,
            peer_asn: 65001,
            ..BgpConfig::default()
        };
        let a = BgpConfig {
            peer_ip: "10.0.0.1".parse().unwrap(),
            ..base.clone()
        };
        let b = BgpConfig {
            peer_ip: "10.0.0.2".parse().unwrap(),
            ..base
        };
        let id_a = BgpSession::from_config(&a).router_id;
        let id_b = BgpSession::from_config(&b).router_id;
        assert_ne!(id_a, id_b);
        assert_ne!(id_a, 0);
        assert_ne!(id_b, 0);
        assert_eq!(id_a >> 16, 0x0AFF);
        assert_eq!(id_b >> 16, 0x0AFF);

        let explicit = BgpConfig {
            router_id: 0x0A00_0001,
            ..a
        };
        assert_eq!(BgpSession::from_config(&explicit).router_id, 0x0A00_0001);
    }

    #[tokio::test]
    async fn run_keepalive_stops_when_sender_dropped() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        drop(tx);
        let mut session = BgpSession {
            connected: true,
            ..BgpSession::default()
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            session.run_keepalive(60, rx),
        )
        .await;
        assert!(matches!(result, Ok(Ok(()))), "dropped sender must end loop");
        assert!(!session.is_connected());
    }

    #[tokio::test]
    async fn run_keepalive_stops_on_true() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        tx.send(true).unwrap();
        let mut session = BgpSession::default();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            session.run_keepalive(60, rx),
        )
        .await;
        assert!(matches!(result, Ok(Ok(()))));
    }

    #[tokio::test]
    async fn write_timeout_aborts_pending_write() {
        // Duplex buffer holds 8 bytes; a larger write can never drain.
        let (mut writer, _reader) = tokio::io::duplex(8);
        let buf = vec![0u8; 4096];
        let result =
            super::write_all_with_timeout(&mut writer, &buf, std::time::Duration::from_millis(50))
                .await;
        assert!(
            matches!(result, Err(crate::BgpError::Session(ref m)) if m == "write timeout"),
            "{result:?}"
        );
    }
}

#[cfg(test)]
mod remediation_tests {
    use super::BgpSession;
    use crate::config::BgpConfig;

    #[test]
    fn explicit_keepalive_is_bounded_by_negotiated_hold_time() {
        let cfg = BgpConfig { hold_time: 90, keepalive_time: 20, ..BgpConfig::default() };
        let mut session = BgpSession::from_config(&cfg);
        session.hold_time = 9;
        assert_eq!(session.negotiated_keepalive(), 3);
    }

    #[test]
    fn zero_negotiated_hold_time_disables_override() {
        let cfg = BgpConfig { hold_time: 90, keepalive_time: 20, ..BgpConfig::default() };
        let mut session = BgpSession::from_config(&cfg);
        session.hold_time = 0;
        assert_eq!(session.negotiated_keepalive(), 0);
    }

    #[test]
    fn ibgp_without_explicit_router_id_does_not_invent_peer_identity() {
        let cfg = BgpConfig { local_asn: 65001, peer_asn: 65001, router_id: 0, ..BgpConfig::default() };
        assert_eq!(BgpSession::from_config(&cfg).router_id, 0);
    }
}
