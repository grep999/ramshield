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
    let ResolvedIpcAuth {
        keys: live_keys,
        roles: role_map,
    } = match resolve_live_auth(&config) {
        Ok(auth) => auth,
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
        let n = match read_chunk(&mut socket, &mut chunk, &config).await? {
            Some(n) => n,
            None => return Ok(()),
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
            let parsed = parse_frame(
                &frame,
                auth_enforced,
                &live_keys,
                &role_map,
                &config,
                &engine,
                &mut socket,
            )
            .await;
            let (req, principal) = match parsed {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(()) => return Ok(()),
            };

            engine.metrics.inc_requests();
            let resp = process_request(
                req,
                principal.as_ref(),
                &engine,
                &event_tx,
                &store,
                &enforcement_tx,
                &dropped_events,
            );
            if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                .await
                .is_err()
            {
                debug!("Write timeout");
                return Ok(());
            }
        }

        // The tail still waiting for its newline is a line that has not ended
        // yet, so it is subject to the line limit — and it is the only part
        // of the buffer that is.
        //
        // Checking the WHOLE buffer would be wrong: the buffer can hold
        // several complete pipelined frames, so its length is not the length
        // of any one line. After the loop above every complete line has been
        // split off, so what remains IS the unterminated tail.
        //
        // The check only applies when the line limit is the TIGHTER of the
        // two bounds. By default `max_line_length` equals
        // `max_connection_bytes` (both 1 MiB), and the connection byte budget
        // is then the binding constraint: it is checked at the top of the
        // loop and the client is told the byte budget was exceeded, which is
        // the accurate answer and the one the oversize-connection test pins.
        // With a line limit configured below the byte budget, this check is
        // what stops an unterminated line from buffering past its limit.
        if config.max_line_length < config.max_bytes && buf.len() > config.max_line_length {
            warn!(
                "Unterminated line exceeds max length ({}), dropping connection",
                config.max_line_length
            );
            let resp = Response::Error {
                code: 413,
                message: format!("line exceeds max_line_length ({})", config.max_line_length),
            };
            let _ = timeout(config.write_timeout, write_resp(&mut socket, &resp)).await;
            return Ok(());
        }

        // Buffer-bloat guard: split_to advances the start pointer but the
        // capacity stays at the high-water mark for the connection's lifetime.
        // Once the drain loop has emptied the buffer, hand the big allocation
        // back instead of holding it against a connection that idles — a
        // client that once pipelined 1 MiB must not pin ~1-2 MiB of capacity
        // for its remaining lifetime. Re-arm at the initial 8 KiB.
        if buf.is_empty() && buf.capacity() > 8192 {
            buf = BytesMut::with_capacity(8192);
        }
    }
}

/// Snapshot the configured authentication keys and role assignments once per connection.
/// Clients reconnect to pick up rotated keys; unspecified roles default to Telemetry later.
struct ResolvedIpcAuth {
    keys: Vec<(String, Vec<u8>)>,
    roles: HashMap<String, KeyRole>,
}

fn resolve_live_auth(config: &ConnectionConfig) -> Result<ResolvedIpcAuth, String> {
    let cfg = config.config.load();
    let keys = parse_ipc_keys(&cfg)?;
    let roles = cfg
        .ipc
        .key_roles
        .iter()
        .map(|key_role| (key_role.key_id.clone(), key_role.role))
        .collect();
    Ok(ResolvedIpcAuth { keys, roles })
}

/// Authenticate and deserialize a frame. A rejected frame is `Ok(None)` when its
/// error response was written; `Err(())` means the response write timed out.
async fn parse_frame(
    frame: &[u8],
    auth_enforced: bool,
    live_keys: &[(String, Vec<u8>)],
    role_map: &HashMap<String, KeyRole>,
    config: &ConnectionConfig,
    engine: &Engine,
    socket: &mut TcpStream,
) -> Result<Option<(Request, Option<Principal>)>, ()> {
    if !auth_enforced {
        return match serde_json::from_slice(frame) {
            Ok(request) => Ok(Some((request, None))),
            Err(error) => {
                trace!(error = %error, frame_bytes = frame.len(), "frame rejected: unparseable Request");
                engine.metrics.inc_frames_rejected();
                let response = Response::Error {
                    code: 1,
                    message: format!("parse: {error}"),
                };
                if timeout(config.write_timeout, write_resp(socket, &response))
                    .await
                    .is_err()
                {
                    debug!("Write timeout on error response");
                    Err(())
                } else {
                    Ok(None)
                }
            }
        };
    }

    // The authentication object remains outside Request so its wire contract is unchanged.
    match verify_frame_auth(live_keys, frame, &config.replay_store) {
        Ok((value, key_id)) => match serde_json::from_value::<Request>(value) {
            Ok(request) => {
                let role = role_map.get(&key_id).copied().unwrap_or(KeyRole::Telemetry);
                Ok(Some((request, Some(Principal { key_id, role }))))
            }
            Err(error) => {
                trace!(error = %error, frame_bytes = frame.len(), "frame rejected: auth-stripped payload does not match Request");
                engine.metrics.inc_frames_rejected();
                let response = Response::Error {
                    code: 1,
                    message: format!("parse: {error}"),
                };
                if timeout(config.write_timeout, write_resp(socket, &response))
                    .await
                    .is_err()
                {
                    Err(())
                } else {
                    Ok(None)
                }
            }
        },
        Err(reason) => {
            warn!("IPC auth rejected: {}", reason);
            engine.metrics.inc_rejected(1);
            engine.metrics.inc_ipc_auth_rejections(1);
            let response = Response::Error {
                code: 401,
                message: format!("unauthorized: {}", reason),
            };
            if timeout(config.write_timeout, write_resp(socket, &response))
                .await
                .is_err()
            {
                Err(())
            } else {
                Ok(None)
            }
        }
    }
}

/// Read at most 2 MiB at a time, preserving the connection's configured read timeout.
/// `None` means the read timed out; EOF remains `Some(0)` for the caller to handle.
async fn read_chunk(
    socket: &mut TcpStream,
    chunk: &mut [u8],
    config: &ConnectionConfig,
) -> Result<Option<usize>, std::io::Error> {
    match timeout(
        config.read_timeout,
        (&mut *socket).take(2 * 1024 * 1024).read(chunk),
    )
    .await
    {
        Ok(Ok(n)) => Ok(Some(n)),
        Ok(Err(error)) => Err(error),
        Err(_) => {
            debug!("Read timeout");
            Ok(None)
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
