use super::*;

/// Semantic shedding classifier (Vuln 2): events that are routine 200 OKs with
/// benign fingerprint and small payload are LOW-signal — shed first at the
/// high-water mark to preserve channel space for attack telemetry.
# File: src/ipc/server/handlers.rs (T19 implementation)
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use crossbeam_channel::Sender;

use crate::engine::Engine;
use ramshield_protocol::message::{Request, Response, IpDetail, Stats};
use ramshield_types::{ConnectionEvent, EnforceAction, EnforceCommand, BlockReason, EvidenceSource};

/// Priority dispatcher for IPC requests.
///
/// Prevents T19 telemetry starvation by isolating low-priority, high-volume
/// connection reports from critical, authoritative control-plane commands.
pub async fn dispatch_request(
    req: Request,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
    enforce_tx: mpsc::Sender<EnforceCommand>,
    actor_id: String,
) -> Result<Response, (u16, &'static str)> {
    match req {
        // =====================================================================
        // PRIORITY 1: CONTROL & HEALTH COMMANDS (Loss-Intolerant)
        // Handled immediately; never blocked behind telemetry backpressure.
        // =====================================================================
        Request::CheckIp(ip) => {
            let store = &engine.store;
            let is_blocked = store.is_blocked(&ip) || store.is_blocked_by_cidr(&ip);
            let threat = store.get_threat_score(&ip).unwrap_or(0.0);
            let ewma = store.get_ewma_rps(&ip).unwrap_or(0.0);
            
            Ok(Response::IpStatus {
                ip,
                blocked: is_blocked,
                threat,
                ewma_rps: ewma,
                reason: "queried".to_string(),
            })
        }

        Request::Block(ip) => {
            let cmd = EnforceCommand {
                id: uuid::Uuid::new_v4(),
                action: EnforceAction::Block,
                evidence_source: EvidenceSource::Operator,
                ip,
                ttl_secs: Some(300), // Default 5 min
                reason: BlockReason::Manual,
                source_ip: ip,
                source_port: 0,
                actor: actor_id,
            };

            // Attempt delivery into enforcement channel with backpressure check
            match enforce_tx.try_send(cmd) {
                Ok(_) => Ok(Response::Ok { message: format!("blocked {ip}") }),
                Err(mpsc::error::TrySendError::Full(_)) => {
                    engine.metrics.inc_enforcement_queue_full();
                    tracing::error!("enforcement channel full: dropping manual block for {ip}");
                    Err((503, "enforcement queue full; system under extreme load"))
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    tracing::error!("enforcement channel closed; daemon shutting down");
                    Err((500, "enforcement actor terminated"))
                }
            }
        }

        Request::Unblock(ip) => {
            let cmd = EnforceCommand {
                id: uuid::Uuid::new_v4(),
                action: EnforceAction::Unblock,
                evidence_source: EvidenceSource::Operator,
                ip,
                ttl_secs: None,
                reason: BlockReason::Manual,
                source_ip: ip,
                source_port: 0,
                actor: actor_id,
            };

            enforce_tx.send(cmd).await.map_err(|_| (500, "enforcement actor dead"))?;
            Ok(Response::Unblocked)
        }

        Request::Stats => {
            let snap = engine.dashboard_snapshot();
            Ok(Response::StatsResponse(Stats {
                blocked_ips: snap.blocked_total,
                total_events: snap.events_ingested,
                rps: snap.rps_current,
                uptime_secs: snap.uptime_secs,
            }))
        }

        Request::IpInfo(ip) => {
            if let Some(record) = engine.store.get_record(&ip) {
                Ok(Response::IpInfoResponse(IpDetail {
                    ip: ip.to_string(),
                    threat_score: record.threat_score,
                    event_count: record.event_count,
                    blocked: record.block_state.is_blocked(),
                    first_seen: record.first_seen_ns / 1_000_000_000,
                    last_seen: record.last_seen_ns / 1_000_000_000,
                }))
            } else {
                Err((404, "ip not tracked in active memory"))
            }
        }

        // =====================================================================
        // PRIORITY 2: BULK INGEST TELEMETRY (Loss-Tolerant, Non-Blocking)
        // Drops or sheds events before allowing channel starvation.
        // =====================================================================
        Request::Connection(report) => {
            let src_ip = report.ip.parse().map_err(|_| (400, "invalid src_ip"))?;
            let upstream_ip = report.upstream_ip.parse().map_err(|_| (400, "invalid upstream_ip"))?;
            
            let event = ConnectionEvent {
                src_ip,
                upstream_ip,
                status: report.status,
                response_time_ms: report.response_time_ms,
                timestamp_ns: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0),
            };

            // Use non-blocking try_send into the crossbeam ingestion ring
            match event_tx.try_send(event) {
                Ok(_) => {
                    engine.metrics.inc_ingested();
                    Ok(Response::Ok { message: "accepted".into() })
                }
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    engine.metrics.inc_shed();
                    Err((429, "ingest queue saturated; telemetry dropped"))
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                    Err((500, "detection engine offline"))
                }
            }
        }
    }
}
pub fn is_low_signal(status_code: u16, proto_fp: u32, bytes: u64) -> bool {
    status_code < 400 && proto_fp == 0 && bytes <= 65_536
}

/// IPC channel high-water mark: 75% of CHANNEL_CAPACITY (64k).
const SHED_WATERMARK: usize = (CHANNEL_CAPACITY as usize * 3) / 4;

pub(crate) fn process_request(
    req: Request,
    principal: Option<&Principal>,
    engine: &Arc<Engine>,
    event_tx: &Sender<ConnectionEvent>,
    store: &Store,
    enforcement_tx: &mpsc::Sender<EnforceCommand>,
    dropped_events: Arc<AtomicU64>,
) -> Response {
    // P2: centralized authorization before request dispatch.
    // Authentication answers "which key?"; Authorization answers "what may it do?".
    // Keep the two separate — enforcement module does not contain authorization logic.
    if let Some(p) = principal
        && let Err(e) = authorize(p, &req)
    {
        engine.metrics.inc_rejected(1);
        engine.metrics.inc_ipc_authz_rejections(1);
        return Response::Error {
            code: 403,
            message: e,
        };
    }

    // P1: attribute enforcement commands to the authenticated principal.
    // When no principal is available (open loopback / unauthenticated path),
    // fall back to the historical hard-coded actor so behaviour is unchanged
    // for the zero-config dev path. P2 closes that gap with real authorization.
    let context = RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events: &dropped_events,
        actor: principal
            .map(|p| p.key_id.clone())
            .unwrap_or_else(|| "admin".to_string()),
    };

    match req {
        request @ Request::CheckIp { .. } => handle_check_ip(request, &context),
        request @ Request::BlockIp { .. } => handle_block_ip(request, &context),
        request @ Request::BlockCidr { .. } => handle_block_cidr(request, &context),
        request @ Request::UnblockIp { .. } => handle_unblock_ip(request, &context),
        request @ Request::UnblockCidr { .. } => handle_unblock_cidr(request, &context),
        request @ Request::GetIpStats { .. } => handle_get_ip_stats(request, &context),
        request @ Request::GetStats => handle_get_stats(request, &context),
        request @ Request::GetStatus => handle_get_status(request, &context),
        request @ Request::ReportConnection { .. } => handle_report_connection(request, &context),
        request @ Request::ReportConnections { .. } => handle_report_connections(request, &context),
        request @ Request::Flush => handle_flush(request, &context),
    }
}

struct RequestContext<'a> {
    engine: &'a Arc<Engine>,
    event_tx: &'a Sender<ConnectionEvent>,
    store: &'a Store,
    enforcement_tx: &'a mpsc::Sender<EnforceCommand>,
    dropped_events: &'a Arc<AtomicU64>,
    actor: String,
}

fn handle_check_ip(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::CheckIp { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            // The CIDR decision is the block; per-IP records are views of it. A member
            // of an active /24 has no IpRecord of its own (subnet_batch enforces
            // at the prefix level), so per-IP state alone reports clean for an
            // address the dataplane is dropping. Same store, same clock the
            // enforcement actor writes — not a second source of truth.
            let cidr_hit = store.is_blocked_by_cidr(&ip_addr);
            let (blocked, threat, ewma_rps, reason) = match store.get(&ip_addr) {
                Some(crate::storage::Value::IpRecord(rec)) => {
                    let per_ip_reason = match rec.block_state {
                        crate::storage::BlockState::Blocked { ref reason, .. } => {
                            Some(reason.as_str().to_string())
                        }
                        _ => None,
                    };
                    let reason =
                        per_ip_reason.or_else(|| cidr_hit.map(|net| format!("cidr_block({net})")));
                    (reason.is_some(), rec.threat_score, rec.ewma_rps, reason)
                }
                _ => (
                    cidr_hit.is_some(),
                    0.0,
                    0.0,
                    cidr_hit.map(|net| format!("cidr_block({net})")),
                ),
            };
            Response::IpStatus {
                ip,
                blocked,
                threat,
                ewma_rps,
                reason,
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_block_ip(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::BlockIp {
            ip,
            reason,
            ttl_secs,
        } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            let reason_display = if reason.is_empty() {
                "manual_block".to_string()
            } else {
                reason.clone()
            };
            let ttl_secs = match sanitize_ttl(ttl_secs) {
                Ok(t) => t,
                Err(msg) => {
                    return Response::Error {
                        code: 400,
                        message: msg,
                    };
                }
            };
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: actor.clone(),
                timestamp_utc: now_ms() as i64 / 1000,
                ttl_seconds: ttl_secs,
                reason,
                ip: ip_addr,
                cidr: None,
                action: EnforceAction::Block,
                evidence_source: ramshield_types::EvidenceSource::Operator,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => {
                    engine
                        .metrics
                        .record_block_ip(&ip_addr, &reason_display, "ipc");
                    Response::Ok {
                        message: format!("block queued for {}", ip_addr),
                        state: Some("pending".into()),
                    }
                }
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_block_cidr(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::BlockCidr {
            cidr,
            reason,
            ttl_secs,
        } => {
            let network = match cidr.parse::<IpNetwork>() {
                Ok(network) => network,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid CIDR: {}", cidr),
                    };
                }
            };
            let ttl_secs = match sanitize_ttl(ttl_secs) {
                Ok(ttl) => ttl,
                Err(msg) => {
                    return Response::Error {
                        code: 400,
                        message: msg,
                    };
                }
            };
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: actor.clone(),
                timestamp_utc: now_ms() as i64 / 1000,
                ttl_seconds: ttl_secs,
                reason: if reason.is_empty() {
                    "manual_cidr_block".into()
                } else {
                    reason
                },
                ip: network.addr,
                cidr: Some(network),
                action: EnforceAction::Block,
                evidence_source: ramshield_types::EvidenceSource::Operator,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => Response::Ok {
                    message: format!("CIDR block queued for {}", network),
                    state: Some("pending".into()),
                },
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_unblock_ip(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::UnblockIp { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: actor.clone(),
                timestamp_utc: now_ms() as i64 / 1000,
                ttl_seconds: 0,
                reason: "manual_unblock".into(),
                ip: ip_addr,
                cidr: None,
                action: EnforceAction::Unblock,
                evidence_source: ramshield_types::EvidenceSource::Operator,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => Response::Ok {
                    message: format!("unblock queued for {}", ip_addr),
                    state: Some("pending".into()),
                },
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_unblock_cidr(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::UnblockCidr { cidr } => {
            let network = match cidr.parse::<IpNetwork>() {
                Ok(network) => network,
                Err(e) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid CIDR: {} ({e})", cidr),
                    };
                }
            };
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: actor.clone(),
                timestamp_utc: now_ms() as i64 / 1000,
                ttl_seconds: 0,
                reason: "manual_unblock".into(),
                ip: network.addr,
                cidr: Some(network),
                action: EnforceAction::Unblock,
                evidence_source: ramshield_types::EvidenceSource::Operator,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => Response::Ok {
                    message: format!("CIDR unblock queued for {network}"),
                    state: Some("pending".into()),
                },
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_get_ip_stats(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::GetIpStats { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            if let Some(crate::storage::Value::IpRecord(rec)) = store.get(&ip_addr) {
                Response::IpDetail(crate::ipc::IpDetail {
                    ip,
                    count: rec.request_count,
                    ewma_rps: rec.ewma_rps,
                    threat: rec.threat_score,
                    state: format!("{:?}", rec.block_state),
                    bytes_in: rec.bytes_in,
                    first_seen_s: rec.first_seen_ns / 1_000_000_000,
                    last_seen_s: rec.last_seen_ns / 1_000_000_000,
                })
            } else {
                Response::IpDetail(crate::ipc::IpDetail {
                    ip,
                    count: 0,
                    ewma_rps: 0.0,
                    threat: 0.0,
                    state: "not_tracked".into(),
                    bytes_in: 0,
                    first_seen_s: 0,
                    last_seen_s: 0,
                })
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_get_stats(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::GetStats => {
            let stats = store.get_stats();
            Response::Stats(crate::ipc::Stats {
                ips_tracked: stats.ips_tracked,
                blocked: stats.blocked,
                ram_bytes: stats.ram_bytes,
                ram_limit_mb: stats.ram_limit_mb,
                uptime_secs: stats.uptime_secs,
                evictions: stats.evictions,
            })
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_get_status(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::GetStatus => Response::Ok {
            message: "ok".into(),
            state: None,
        },
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_report_connection(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::ReportConnection {
            ip,
            bytes,
            status_code,
            proto_fp,
            l7,
            http_request: _,
        } => {
            let ev = ConnectionEvent {
                ip: match ip.parse() {
                    Ok(addr) => addr,
                    Err(_) => {
                        return Response::Error {
                            code: 400,
                            message: format!("invalid ip address: {}", ip),
                        };
                    }
                },
                timestamp_ns: now_ms() * 1_000_000,
                bytes,
                status_code,
                proto_fingerprint: proto_fp,
                l7,
            };
            // STAGE 1: Semantic shedding — at >=75% occupancy, shed low-signal
            // (routine 200 OK / benign fingerprint) to preserve space for
            // attack telemetry (401/429/500, anomalous fp, >64 KiB).
            let is_low_signal = is_low_signal(status_code, proto_fp, bytes);
            if event_tx.len() >= SHED_WATERMARK && is_low_signal {
                engine.metrics.inc_shed(1);
                trace!(
                    ip = %ip,
                    status_code,
                    proto_fp,
                    bytes,
                    chan_depth = event_tx.len(),
                    "event shed: low-signal at watermark"
                );
                return Response::BatchOk {
                    accepted: 0,
                    rejected: 0,
                };
            }
            match event_tx.try_send(ev) {
                Ok(()) => Response::Ok {
                    message: "accepted".into(),
                    state: None,
                },
                Err(_) => {
                    dropped_events.fetch_add(1, Ordering::Relaxed);
                    // P1 fix (F2): local counter had no consumers — dashboard
                    // saw zero drops exactly when the channel saturated.
                    engine.metrics.inc_rejected(1);
                    engine.metrics.inc_ipc_event_drops(1);
                    Response::BatchOk {
                        accepted: 0,
                        rejected: 1,
                    }
                }
            }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_report_connections(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::ReportConnections { events } => {
            let now = now_ms() * 1_000_000;
            let total = events.len() as u32;
            let mut accepted = 0u32;
            let mut rejected = 0u32;
            let mut shed = 0u32;
            let mut tail_dropped = 0u32;
            for cr in events {
                let ev = ConnectionEvent {
                    ip: cr.ip, // typed on the wire (item 2): no per-event String alloc + parse
                    timestamp_ns: now,
                    bytes: cr.bytes,
                    status_code: cr.status_code,
                    proto_fingerprint: cr.proto_fp,
                    l7: cr.l7,
                };
                // STAGE 1: Semantic shedding — at >=75% occupancy, shed low-signal
                // (routine 200 OK / benign fingerprint) to preserve space for
                // attack telemetry (401/429/500, anomalous fp, >64 KiB).
                if event_tx.len() >= SHED_WATERMARK
                    && is_low_signal(cr.status_code, cr.proto_fp, cr.bytes)
                {
                    engine.metrics.inc_shed(1);
                    shed += 1;
                    trace!(
                        ip = %cr.ip,
                        status_code = cr.status_code,
                        proto_fp = cr.proto_fp,
                        bytes = cr.bytes,
                        chan_depth = event_tx.len(),
                        "batch event shed: low-signal at watermark"
                    );
                    continue; // shed: do not enqueue
                }
                match event_tx.try_send(ev) {
                    Ok(()) => accepted += 1,
                    Err(_) => {
                        // STAGE 2: Emergency full — fall back to existing drop logic
                        rejected += 1;
                        dropped_events.fetch_add(1, Ordering::Relaxed);
                        engine.metrics.inc_rejected(1); // F2
                        engine.metrics.inc_ipc_event_drops(1);
                        trace!(
                            ip = %cr.ip,
                            rejected,
                            chan_depth = event_tx.len(),
                            "batch event rejected: channel full"
                        );
                        // Sampled: per-event debug here floods under
                        // backpressure (this was one line per rejected event).
                        if rejected & 0x3FF == 1 {
                            debug!(
                                rejected,
                                chan_depth = event_tx.len(),
                                "event channel full (sampled 1/1024)"
                            );
                        }
                    }
                }
                if accepted + rejected >= BATCH_MAX as u32 && total > accepted + rejected {
                    let dropped = total - accepted - rejected;
                    rejected += dropped;
                    tail_dropped = dropped;
                    dropped_events.fetch_add(dropped as u64, Ordering::Relaxed);
                    engine.metrics.inc_rejected(dropped as u64); // F2
                    engine.metrics.inc_ipc_event_drops(dropped as u64);
                    trace!(
                        tail_dropped = dropped,
                        processed = accepted + rejected - dropped,
                        total,
                        "batch tail dropped at BATCH_MAX"
                    );
                    break;
                }
            }
            // One DEBUG line per frame is a flood vector under attack
            // (measured: 382/s during a 120-frame barrage). Log every 64th
            // batch and always log when the batch lost events — anomalies must
            // never be sampled away.
            static BATCH_LOG_SEQ: AtomicU64 = AtomicU64::new(0);
            let seq = BATCH_LOG_SEQ.fetch_add(1, Ordering::Relaxed);
            if seq.is_multiple_of(64) || shed > 0 || tail_dropped > 0 || rejected > 0 {
                debug!(
                    batch_seq = seq,
                    samples = total,
                    accepted,
                    rejected,
                    shed,
                    tail_dropped,
                    chan_depth = event_tx.len(),
                    "report_connections batch"
                );
            }
            Response::BatchOk { accepted, rejected }
        }
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}

fn handle_flush(req: Request, ctx: &RequestContext<'_>) -> Response {
    #[allow(unused_variables)]
    let RequestContext {
        engine,
        event_tx,
        store,
        enforcement_tx,
        dropped_events,
        actor,
    } = ctx;
    match req {
        Request::Flush => Response::Ok {
            message: "no-op: flush is automatic (pre_aggs window)".into(),
            state: None,
        },
        _ => unreachable!("request variant dispatched to the wrong handler"),
    }
}
