// ── Binary: stdin/TCP event ingest → detection → nftables enforcement ───────
//
// Event format: one JSON object per line.
//   {"ip":"203.0.113.9","source":"sshd","success":false,"username":"root"}
// Feed from journald:  journalctl -f -u ssh | jq -c '...' | bruteforce-monitor
// Or listen:           bruteforce-monitor --listen 127.0.0.1:9400
//
// Enforcement (must exist before first block):
//   nft add table inet bruteforce
//   nft add set inet bruteforce blocklist4 '{ type ipv4_addr; flags timeout; }'
//   nft add set inet bruteforce blocklist6 '{ type ipv6_addr; flags timeout; }'
//   nft add rule inet bruteforce input ip saddr @blocklist4 drop
//   nft add rule inet bruteforce input ip6 saddr @blocklist6 drop

use std::env;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, TcpListener};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ramforge::{AuthEvent, BruteforceMonitor, DetectionConfig};
use serde::Deserialize;

#[derive(Deserialize)]
struct EventLine {
    ip: IpAddr,
    #[serde(default = "default_source")]
    source: String,
    #[serde(default)]
    success: bool,
    username: Option<String>,
}

fn default_source() -> String {
    "unknown".into()
}

fn handle_line(line: &str, monitor: &BruteforceMonitor) {
    match serde_json::from_str::<EventLine>(line) {
        Ok(e) if !e.success => {
            let evt = AuthEvent {
                ip: e.ip,
                timestamp: Instant::now(),
                source: e.source,
                success: false,
                username: e.username,
            };
            monitor.process_failure(&evt);
        }
        Ok(_) => {} // successes never count toward the counter
        Err(err) => tracing::warn!(line = %line, %err, "unparseable event line"),
    }
}

fn nft_block(ip: IpAddr, ttl_secs: u64, reason: &str) {
    let set = if ip.is_ipv4() { "blocklist4" } else { "blocklist6" };
    match Command::new("nft")
        .args(["add", "element", "inet", "bruteforce", set])
        .arg(format!("{{ {ip} timeout {ttl_secs}s }}"))
        .output()
    {
        Ok(out) if out.status.success() => {
            tracing::info!(%ip, %reason, ttl_secs, "blocked via nftables");
        }
        Ok(out) => {
            tracing::warn!(
                %ip, %reason,
                stderr = %String::from_utf8_lossy(&out.stderr),
                "nft add element failed — block NOT enforced"
            );
        }
        Err(e) => {
            tracing::warn!(%ip, %reason, error = %e, "nftables unavailable — block logged only");
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut listen: Option<String> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next(),
            other => {
                eprintln!("usage: bruteforce-monitor [--listen ADDR:PORT]");
                let _ = other;
                std::process::exit(2);
            }
        }
    }

    let (monitor, block_rx) = BruteforceMonitor::new(DetectionConfig::default());
    let monitor = Arc::new(monitor);

    // Enforcement consumer.
    std::thread::spawn(move || {
        for cmd in block_rx {
            nft_block(cmd.ip, cmd.ttl_secs, cmd.reason.as_str());
        }
    });

    // Stats heartbeat.
    {
        let m = Arc::clone(&monitor);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(60));
                let s = m.snapshot_stats();
                tracing::info!(
                    tracked = s.ips_tracked,
                    blocked = s.currently_blocked,
                    failures = s.total_failures,
                    "stats"
                );
            }
        });
    }

    // Optional TCP ingest.
    if let Some(addr) = listen {
        let listener = TcpListener::bind(&addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
        tracing::info!(%addr, "listening for events");
        let m = Arc::clone(&monitor);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let m = Arc::clone(&m);
                std::thread::spawn(move || {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().map_while(Result::ok) {
                        handle_line(&line, &m);
                    }
                });
            }
        });
    }

    // Stdin ingest (primary: pipe from journalctl/syslog).
    for line in BufReader::new(std::io::stdin()).lines().map_while(Result::ok) {
        handle_line(&line, &monitor);
    }

    monitor.shutdown();
}