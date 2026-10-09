use super::*;

impl ConnectionConfig {
    pub(crate) fn from_server(server: &IpcServer) -> Self {
        Self {
            max_bytes: server.max_connection_bytes,
            read_timeout: Duration::from_millis(server.read_timeout_ms),
            write_timeout: Duration::from_millis(server.write_timeout_ms),
            idle_timeout: Duration::from_millis(server.connection_idle_timeout_ms),
            max_line_length: server.max_line_length,
            config: server.config.clone(),
            replay_store: server.replay_store.clone(),
        }
    }
}

pub(crate) async fn handle_connection(
    mut socket: TcpStream,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
    store: Arc<Store>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    config: ConnectionConfig,
    dropped_events: Arc<AtomicU64>,
) -> Result<(), std::io::Error> {
    let mut buf = BytesMut::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let mut total_bytes_read = 0usize;
    let mut last_activity = Instant::now();

    // Parse auth keys once per connection. Config is validated before storage (api_set_config),
    // so parse never fails here. Clients reconnect to pick up rotated keys.
    // P2: resolve key roles once per connection. Keys not listed default to
    // Telemetry (least privilege) — explicit config always wins over naming.
    let (live_keys, role_map) = {
        let cfg = config.config.load();
        match parse_ipc_keys(&cfg) {
            Ok(k) => {
                let mut roles: HashMap<String, KeyRole> = HashMap::new();
                for kr in &cfg.ipc.key_roles {
                    roles.insert(kr.key_id.clone(), kr.role);
                }
                (k, roles)
            }
            Err(e) => {
                error!(error = %e, "IPC auth keys invalid in live config — closing connection");
                let resp = Response::Error {
                    code: 500,
                    message: "internal configuration error: invalid auth state".into(),
                };
                let _ = timeout(config.write_timeout, write_resp(&mut socket, &resp)).await;
                return Err(std::io::Error::other(format!(
                    "invalid runtime auth keys: {e}"
                )));
            }
        }
    };
    let auth_enforced = !live_keys.is_empty();

    loop {
        if last_activity.elapsed() > config.idle_timeout {
            debug!("Connection idle timeout");
            return Ok(());
        }

        if total_bytes_read >= config.max_bytes {
            debug!("Connection exceeded max bytes ({})", config.max_bytes);
            // F4: tell the client WHY instead of a bare TCP reset. Best-effort —
            // client may already be gone; do not read past the cap to find a newline.
            let resp = Response::Error {
                code: 413,
                message: format!(
                    "connection exceeded max_connection_bytes ({})",
                    config.max_bytes
                ),
            };
            let _ = timeout(config.write_timeout, write_resp(&mut socket, &resp)).await;
            return Ok(());
        }

        // 2 MiB ceiling per read(): a hostile peer streaming without newline
        // cannot make one read() hand us more than 2 MiB. Reborrow — Take<&mut
        // TcpStream> borrows for this expression only; socket stays owned for
        // the write_resp calls below (take(self) would move it).
        let n = match timeout(
            config.read_timeout,
            (&mut socket).take(2 * 1024 * 1024).read(&mut chunk),
        )
        .await
        {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                debug!("Read timeout");
                return Ok(());
            }
        };

        if n == 0 {
            return Ok(());
        }

        total_bytes_read += n;
        last_activity = Instant::now();
        buf.extend_from_slice(&chunk[..n]);

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            if pos > config.max_line_length {
                warn!(
                    "Line exceeds max length ({}), dropping connection",
                    config.max_line_length
                );
                return Ok(());
            }
            // P2 fix: was `buf.drain(..=pos).collect()` — drain shifts the
            // unread tail left, O(bytes-buffered) per line; a client that
            // pipelines a batch pays quadratically in the buffered bytes.
            // BytesMut::split_to is O(1) (advances the start pointer).
            let frame = buf.split_to(pos + 1);
            // P1/P2: both branches yield (Request, Option<Principal>). Auth path
            // supplies the authenticated key_id and its configured role; open-
            // loopback path has none, so enforcement attribution falls back
            // (see process_request).
            let (req, principal) = if auth_enforced {
                // HMAC auth gate: enforced only when keys configured. The auth
                // object rides OUTSIDE the Request enum so deny_unknown_fields
                // on the wire contract stays intact.
                match verify_frame_auth(&live_keys, &frame, &config.replay_store) {
                    // P1-5: verify_frame_auth returns the auth-stripped Value
                    // and the authenticated key_id; from_value deserializes
                    // without a second JSON parse.
                    Ok((v, key_id)) => match serde_json::from_value::<Request>(v) {
                        Ok(req) => {
                            // P2: look up configured role; default to Telemetry
                            // (least privilege). Keys without explicit config get
                            // the default — explicit always wins over naming.
                            let role = role_map.get(&key_id).copied().unwrap_or(KeyRole::Telemetry);
                            (req, Some(Principal { key_id, role }))
                        }
                        Err(e) => {
                            trace!(
                                error = %e,
                                frame_bytes = frame.len(),
                                "frame rejected: auth-stripped payload does not match Request"
                            );
                            engine.metrics.inc_frames_rejected();
                            let resp = Response::Error {
                                code: 1,
                                message: format!("parse: {e}"),
                            };
                            if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                            continue;
                        }
                    },
                    Err(reason) => {
                        // Replay-store saturation/poison is an availability failure,
                        // not an authentication failure. Fail closed without exposing
                        // internal capacity/lock details to an unauthenticated peer.
                        let (code, message) = match reason {
                            "capacity" | "replay store poisoned" => {
                                warn!("IPC auth temporarily unavailable: replay store unavailable");
                                (503, "temporarily unavailable".to_string())
                            }
                            _ => {
                                warn!("IPC auth rejected: {}", reason);
                                (401, "unauthorized".to_string())
                            }
                        };
                        engine.metrics.inc_rejected(1);
                        engine.metrics.inc_ipc_auth_rejections(1);
                        let resp = Response::Error {
                            code,
                            message,
                        };
                        if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                        continue;
                    }
                }
            } else {
                // No auth keys configured - accept all frames
                match serde_json::from_slice(&frame) {
                    Ok(req) => (req, None),
                    Err(e) => {
                        trace!(
                            error = %e,
                            frame_bytes = frame.len(),
                            "frame rejected: unparseable Request"
                        );
                        engine.metrics.inc_frames_rejected();
                        let resp = Response::Error {
                            code: 1,
                            message: format!("parse: {e}"),
                        };
                        if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                            .await
                            .is_err()
                        {
                            debug!("Write timeout on error response");
                            return Ok(());
                        }
                        continue;
                    }
                }
            };

            engine.metrics.inc_requests();
            let resp = process_request(
                req,
                principal.as_ref(),
                &engine,
                &event_tx,
                &store,
                &enforcement_tx,
                dropped_events.clone(),
            );
            if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                .await
                .is_err()
            {
                debug!("Write timeout");
                return Ok(());
            }
        }
    }
}

async fn write_resp(socket: &mut TcpStream, resp: &Response) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec(resp)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    socket.write_all(&bytes).await?;
    socket.write_all(b"\n").await?;
    Ok(())
}
