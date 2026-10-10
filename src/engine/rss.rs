//! RSS concentration guard. XDP executes after NIC RSS selection; a single
//! hot 5-tuple can therefore overload one RX queue before per-CPU XDP budgets
//! can help. ethtool documents `-X ... equal N` as the operation that spreads
//! the RSS indirection table across the first N receive queues.
use std::process::Command;
use tracing::{info, warn};

pub fn rebalance(interface: &str, required: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if cpus <= 1 {
            return Ok(());
        }
        match Command::new("ethtool")
            .args(["-X", interface, "equal", &cpus.to_string()])
            .output()
        {
            Ok(out) if out.status.success() => {
                info!(%interface, cpus, "RSS indirection balanced across CPUs");
                Ok(())
            }
            Ok(out) => {
                let err = format!(
                    "ethtool RSS rebalance failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                if required {
                    Err(err)
                } else {
                    warn!(%interface, error=%err, "RSS rebalance unavailable; continuing");
                    Ok(())
                }
            }
            Err(e) => {
                let err = format!("ethtool unavailable: {e}");
                if required {
                    Err(err)
                } else {
                    warn!(%interface, error=%err, "RSS rebalance unavailable; continuing");
                    Ok(())
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface, required);
        Err("RSS rebalance requires Linux ethtool".into())
    }
}
