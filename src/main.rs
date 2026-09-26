use anyhow::{Context, Result};
use ramshield::{Config, Engine, dashboard};
use std::sync::Arc;
use tracing::info; // Add debug
use tracing_subscriber::EnvFilter;

/// CLI contract: `--config <path>` and a bare positional `<path>` both select
/// the config file. Unknown flags and missing values are FATAL — the old
/// parser silently dropped unrecognized arguments, so `./ramshield config.toml`
/// (positional) booted on compiled-in defaults with the file ignored.
fn parse_args(args: &[String]) -> Result<(Option<String>, bool, bool)> {
    let mut config_path: Option<String> = None;
    let mut no_xdp = false;
    let mut doctor = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config" | "-c" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("--config requires a path"))?;
                if config_path.is_some() {
                    anyhow::bail!("multiple config paths given");
                }
                config_path = Some(v.clone());
                i += 2;
            }
            "--no-xdp" => {
                no_xdp = true;
                i += 1;
            }
            "doctor" => {
                doctor = true;
                i += 1;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                anyhow::bail!("unknown argument: {s}");
            }
            _ => {
                if config_path.is_some() {
                    anyhow::bail!("multiple config paths given");
                }
                config_path = Some(args[i].clone());
                i += 1;
            }
        }
    }
    Ok((config_path, no_xdp, doctor))
}

#[tokio::main]
async fn main() -> Result<()> {
    ramshield::install_panic_hook();
    // Atomic P0: --version flag (BACKLOG #8) — checked before tracing init
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ramshield {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ramshield=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let (config_path, no_xdp, doctor) = parse_args(&args)?;

    let mut config = match config_path {
        Some(path) => {
            let absolute_path = std::fs::canonicalize(&path)
                .map_err(|e| anyhow::anyhow!("Error canonicalizing path {}: {}", path, e))?;
            eprintln!(
                "Attempting to load config from absolute path: {:?}",
                absolute_path
            );
            Config::load(
                absolute_path
                    .to_str()
                    .context("config path contains non-UTF-8 characters")?,
            )?
        }
        None => {
            // Still honor env overrides in no-config mode (dashboard auth etc).
            let mut c = Config::default();
            c.apply_env_overrides()?;
            // P1 fix (same class as Config::load): env overrides could set a
            // public bind with no secrets; the fail-closed guard must run on
            // the FINAL config here too, not just on the file path.
            c.validate()?;
            c
        }
    };
    // --no-xdp: run the detect/block pipeline without attaching the XDP program.
    if no_xdp {
        config.xdp.enabled = false;
    }
    // ponytail: Config::load() already calls apply_env_overrides() once
    // internally. A second call here was harmless (idempotent) but wasteful
    // and confusing — removed.
    // ponytail: Debug on Config leaks auth_keys. Print summary, not raw.
    info!(
        "Loaded config: ipc.auth_keys={}, dashboard.bind={}",
        config.ipc.auth_keys.len(),
        config.dashboard.http_addr
    );
    // P1-7: no TLS in the stack — surface any public-bind exposure at boot.
    for w in config.exposure_warnings() {
        tracing::warn!("{w}");
    }

    if doctor {
        run_doctor();
        return Ok(());
    }

    // Start RamShield normally
    let store = Arc::new(ramshield::storage::Store::new(config.engine.shard_count));
    store.traffic.ram_limit_mb.store(
        config.engine.ram_limit_mb,
        std::sync::atomic::Ordering::Relaxed,
    );
    // Store created_at for uptime tracking
    store
        .traffic
        .uptime_secs
        .store(1, std::sync::atomic::Ordering::Relaxed); // mark non-zero
    let metrics = Arc::new(ramshield::metrics::Metrics::with_block_log(
        config.dashboard.block_log_size,
    ));
    let engine = Arc::new(Engine::new(config.clone(), store.clone(), metrics.clone()));
    let _engine_handle = engine
        .clone()
        .start_async()
        .context("failed to start engine pipeline")?;

    // Periodic uptime updater (every second)
    {
        let started = std::time::Instant::now();
        let traffic = store.traffic.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                traffic.uptime_secs.store(
                    started.elapsed().as_secs(),
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
        });
    }

    // Start dashboard if enabled — dedicated OS thread + tokio runtime
    // to guarantee responsiveness under detection load.
    let eng_clone = engine.clone();
    let dashboard_config = config.dashboard.clone();
    let config_clone = config.clone();
    if dashboard_config.enabled {
        let _dashboard_handle = std::thread::Builder::new()
            .name("rs-dashboard".into())
            .spawn(move || -> Result<()> {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                    .context("failed to build dashboard runtime")?;
                rt.block_on(async move {
                    if let Err(e) =
                        dashboard::serve(eng_clone, &dashboard_config.http_addr, &config_clone)
                            .await
                    {
                        tracing::error!("Dashboard server error: {}", e);
                    }
                });
                Ok(())
            })
            .context("failed to spawn dashboard thread")?;
    }

    info!("RamShield running — Ctrl+C to stop");

    // Graceful shutdown trap: SIGINT (Ctrl+C), SIGTERM (systemd stop/kill
    // default), SIGHUP (terminal hangup / reload request). Any of the three
    // starts the same drain: engine.shutdown() → worker joins → enforcement
    // task exits → AyaXdpApplier dropped → Ebpf closed → kernel unbinds the
    // XDP program (RAII detach restores default stack forwarding).
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    tokio::select! {
        _ = sigint.recv() => info!("Received SIGINT; initiating graceful shutdown"),
        _ = sigterm.recv() => info!("Received SIGTERM; initiating graceful shutdown"),
        _ = sighup.recv() => info!("Received SIGHUP; initiating graceful shutdown"),
    }

    // Initiate graceful shutdown
    engine.shutdown();

    // F9: real joins (workers final-flush pre_aggs on exit) with a 5s grace,
    // instead of a fixed spin that neither guaranteed completion nor early-exit.
    // Blocking joins go through spawn_blocking — must not park the RT.
    let eng = engine.clone();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        tokio::task::spawn_blocking(move || eng.join_workers(std::time::Duration::from_secs(5))),
    )
    .await;

    info!("Shutdown complete.");
    Ok(())
}

/// Check host readiness non-interactively. Exit 0 if all gates pass.
fn run_doctor() {
    let mut problems: Vec<String> = Vec::new();
    // kernel
    let krn = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .map(|o| String::from_utf8(o.stdout).unwrap_or_else(|_| "unknown".into()))
        .unwrap_or("unknown".into());
    info!("doctor: kernel {krn}");
    // config
    let cfg_path = std::env::var("RAMSHIELD_DOCTOR_CONFIG").unwrap_or("config.toml".into());
    let cfg = Config::load(cfg_path.as_str()).ok();
    if let Some(c) = cfg {
        let wal_dir = c.wal.dir;
        if std::path::Path::new(&wal_dir).exists() {
            info!("doctor: WAL dir {wal_dir} exists");
        } else {
            problems.push(format!("WAL dir {wal_dir}: not found"));
        }
        if !c.ipc.auth_keys.is_empty() {
            info!("doctor: IPC auth configured");
        } else {
            problems.push("IPC auth_keys empty".into());
        }
        let iface = c.xdp.interface;
        let iface_ok =
            std::process::Command::new("ip")
                .arg("link").arg("show").arg(iface.clone())
                .output().is_ok();
        if iface_ok {
            info!("doctor: XDP interface {iface} exists");
        } else {
            problems.push(format!("XDP interface {iface}: not found"));
        }
    } else {
        problems.push(format!("config load failed: {cfg_path}"));
    }
    // caps
    let bin = std::env::args().next().unwrap_or("ramshield".into());
    let caps = std::process::Command::new("getcap")
        .arg(bin)
        .output()
        .map(|o| String::from_utf8(o.stdout).unwrap_or_default())
        .unwrap_or("".into());
    if caps.contains("cap_bpf") || caps.contains("cap_net_admin") {
        info!("doctor: capabilities present");
    } else {
        problems.push("cap_bpf/cap_net_admin not set (needed for XDP)".into());
    }
    if problems.is_empty() {
        info!("doctor: all checks passed");
        std::process::exit(0);
    } else {
        for p in problems {
            tracing::error!("doctor: {p}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod cli_tests {
    use super::parse_args;

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("ramshield".to_string())
            .chain(v.iter().map(|s| s.to_string()))
            .collect()
    }

    fn cfg(v: &[&str]) -> Option<String> {
        parse_args(&args(v)).unwrap().0
    }

    #[test]
    fn config_flag_selects_path() {
        assert_eq!(cfg(&["--config", "a.toml"]), Some("a.toml".into()));
        assert_eq!(cfg(&["-c", "a.toml"]), Some("a.toml".into()));
    }

    #[test]
    fn positional_path_selects_config() {
        // Regression: the old parser silently dropped this — the file was
        // never loaded and the process booted on compiled-in defaults.
        assert_eq!(cfg(&["config.toml"]), Some("config.toml".into()));
    }

    #[test]
    fn no_xdp_flag_is_recognized() {
        // prod_smoke.sh boots with --no-xdp; the parser must not reject it.
        let (path, no_xdp, _doctor) = parse_args(&args(&["--no-xdp", "--config", "a.toml"])).unwrap();
        assert!(no_xdp);
        assert_eq!(path, Some("a.toml".into()));
        assert!(!parse_args(&args(&["--config", "a.toml"])).unwrap().1);
    }

    #[test]
    fn unknown_flag_is_fatal() {
        // Fail-closed: a typo'd flag must not boot with an ignored argument.
        assert!(parse_args(&args(&["--confg", "a.toml"])).is_err());
        assert!(parse_args(&args(&["--x"])).is_err());
    }

    #[test]
    fn missing_flag_value_is_fatal() {
        assert!(parse_args(&args(&["--config"])).is_err());
    }

    #[test]
    fn duplicate_config_path_is_fatal() {
        assert!(parse_args(&args(&["--config", "a.toml", "b.toml"])).is_err());
        assert!(parse_args(&args(&["a.toml", "--config", "b.toml"])).is_err());
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(parse_args(&args(&[])).unwrap().0, None);
    }
}
