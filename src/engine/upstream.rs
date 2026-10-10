use crate::config::Config;
use crate::metrics::Metrics;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, info, warn};

#[derive(Debug, Serialize)]
struct SaturationSignal {
    event: &'static str,
    interface: String,
    observed_bps: u64,
    observed_rps: u64,
    utilization_pct: u8,
    threshold_pct: u8,
    timestamp_ms: u64,
}

pub async fn run(
    config: Arc<Config>,
    metrics: Arc<Metrics>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let cfg = config.upstream.clone();
    if !cfg.enabled {
        return;
    }
    if !cfg
        .webhook_url
        .as_deref()
        .is_some_and(|u| u.starts_with("http://"))
    {
        warn!(
            "upstream mitigation enabled without an http:// webhook; saturation will be detected and logged only"
        );
    }
    let mut tick = tokio::time::interval(Duration::from_millis(cfg.poll_ms.max(250)));
    let mut previous = read_rx_bytes(&cfg.interface);
    let mut previous_ts = Instant::now();
    let mut previous_events = metrics
        .events_ingested
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut last_signal = Instant::now() - Duration::from_secs(cfg.cooldown_secs);
    let mut bgp_announced = false;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let now = Instant::now();
                let current = read_rx_bytes(&cfg.interface);
                let elapsed = now.duration_since(previous_ts).as_secs_f64().max(0.001);
                let events = metrics.events_ingested.load(std::sync::atomic::Ordering::Relaxed);
                let rx_bps = current.saturating_sub(previous) as f64 * 8.0 / elapsed;
                let rps = events.saturating_sub(previous_events) as f64 / elapsed;
                previous = current;
                previous_events = events;
                previous_ts = now;
                let capacity_bps = cfg.link_capacity_mbps as f64 * 1_000_000.0;
                if capacity_bps == 0.0 { continue; }
                let utilization = (rx_bps / capacity_bps * 100.0).min(100.0);
                if utilization < (cfg.saturation_pct.saturating_sub(5)) as f64 && bgp_announced {
                    if let Err(e) = bgp_withdraw(&cfg) { warn!(error=%e, "BGP mitigation withdrawal failed"); } else { bgp_announced = false; }
                }
                // Physical saturation is itself the trigger. A low-packet-rate
                // high-byte flood must not wait for an L7/RPS detector that may
                // never fire before the carrier queue is full.
                if utilization >= cfg.saturation_pct as f64 {
                    let now_ms = crate::metrics::now_ms();
                    let signal = SaturationSignal {
                        event: "uplink_saturation",
                        interface: cfg.interface.clone(),
                        observed_bps: rx_bps.max(0.0) as u64,
                        observed_rps: rps.max(0.0) as u64,
                        utilization_pct: utilization as u8,
                        threshold_pct: cfg.saturation_pct,
                        timestamp_ms: now_ms,
                    };
                    warn!(interface=%signal.interface, utilization_pct=signal.utilization_pct, observed_rps=signal.observed_rps, "upstream saturation detected; local mitigation cannot reclaim an already saturated uplink");
                    if cfg.bgp_mode != "none" && !bgp_announced {
                        match bgp_announce(&cfg) { Ok(()) => { bgp_announced = true; info!(mode=%cfg.bgp_mode, "upstream BGP mitigation announced"); }, Err(e) => warn!(error=%e, "upstream BGP mitigation failed") }
                    }
                    if let Some(url) = &cfg.webhook_url
                        && last_signal.elapsed() >= Duration::from_secs(cfg.cooldown_secs)
                    {
                        match post_webhook(url, &signal).await {
                            Ok(()) => { last_signal = now; debug!("upstream mitigation webhook delivered"); }
                            Err(e) => warn!(error=%e, "upstream mitigation webhook failed"),
                        }
                    }
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
        }
    }
}

fn read_rx_bytes(interface: &str) -> u64 {
    #[cfg(target_os = "linux")]
    {
        let path = format!("/sys/class/net/{interface}/statistics/rx_bytes");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = interface;
        0
    }
}

async fn post_webhook(url: &str, signal: &SaturationSignal) -> Result<(), String> {
    let raw = url.strip_prefix("http://").ok_or_else(|| {
        "upstream webhook currently requires http://; terminate TLS in the deployment proxy"
            .to_string()
    })?;
    let (authority, path) = if let Some((a, p)) = raw.split_once('/') {
        (a, format!("/{p}"))
    } else {
        (raw, "/".to_string())
    };
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, 80u16), |(h, p)| (h, p.parse().unwrap_or(80)));
    let body = serde_json::to_vec(signal).map_err(|e| e.to_string())?;
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| e.to_string())?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(&body).await.map_err(|e| e.to_string())?;
    let mut response = [0u8; 256];
    let n = stream
        .read(&mut response)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 || !response.starts_with(b"HTTP/1.1 2") && !response.starts_with(b"HTTP/1.0 2") {
        return Err("upstream webhook returned a non-2xx response".into());
    }
    Ok(())
}

fn bgp_announce(cfg: &ramshield_config::UpstreamConfig) -> Result<(), String> {
    let fifo = cfg.bgp_fifo.as_deref().ok_or("bgp_fifo missing")?;
    let prefix = cfg
        .protected_prefix
        .as_deref()
        .ok_or("protected_prefix missing")?;
    let command = match cfg.bgp_mode.as_str() {
        "flowspec" => format!(
            "announce flow route {{ match {{ destination {prefix}; }} then {{ discard; }} }}\n"
        ),
        "rtbh" => format!(
            "announce route {prefix} next-hop self community [{}]\n",
            cfg.bgp_community
                .as_deref()
                .ok_or("bgp_community missing")?
        ),
        _ => return Ok(()),
    };
    write_bgp_fifo(fifo, command.as_bytes())
}

fn bgp_withdraw(cfg: &ramshield_config::UpstreamConfig) -> Result<(), String> {
    let fifo = cfg.bgp_fifo.as_deref().ok_or("bgp_fifo missing")?;
    let prefix = cfg
        .protected_prefix
        .as_deref()
        .ok_or("protected_prefix missing")?;
    let command = match cfg.bgp_mode.as_str() {
        "flowspec" => format!("withdraw flow route {{ match {{ destination {prefix}; }} }}\n"),
        "rtbh" => format!("withdraw route {prefix}\n"),
        _ => return Ok(()),
    };
    write_bgp_fifo(fifo, command.as_bytes())
}

fn write_bgp_fifo(path: &str, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let c = std::ffi::CString::new(path).map_err(|_| "bgp_fifo contains NUL".to_string())?;
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(format!(
                "open BGP FIFO: {}",
                std::io::Error::last_os_error()
            ));
        }
        let rc = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        let close_rc = unsafe { libc::close(fd) };
        if rc < 0 {
            return Err(format!(
                "write BGP FIFO: {}",
                std::io::Error::last_os_error()
            ));
        }
        if close_rc < 0 {
            return Err(format!(
                "close BGP FIFO: {}",
                std::io::Error::last_os_error()
            ));
        }
        if rc as usize != bytes.len() {
            return Err("partial BGP FIFO write".into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, bytes);
        Err("BGP FIFO integration requires Linux".into())
    }
}
