use anyhow::{Context, Result};
use ramshield::{Config, Engine, dashboard};
use std::sync::Arc;
use tracing::info; // Add debug
use tracing_subscriber::EnvFilter;

/// CLI parser for the daemon binary. A positional config path remains supported
/// for backwards compatibility; `doctor` is a real clap subcommand.
#[derive(clap::Parser)]
#[command(name = "ramshield", about = "RamShield DDoS defense and XDP engine")]
struct CliArgs {
    /// TOML configuration file.
    #[arg(short, long, global = true)]
    config: Option<String>,
    /// Run without attaching XDP.
    #[arg(long, global = true)]
    no_xdp: bool,
    /// Validate configuration and exit.
    #[arg(long, global = true)]
    validate: bool,
    /// Legacy positional configuration path.
    #[arg(value_name = "CONFIG", hide = true)]
    positional_config: Option<String>,
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(clap::Subcommand)]
enum CliCommand {
    /// Diagnose host readiness, kernel capabilities, configuration, XDP, maps, detectors, and mesh.
    Doctor {
        /// Run only one subsystem: core, config, xdp, maps, stats, or mesh.
        #[arg(long)]
        subsystem: Option<String>,
    },
}

/// Return (config path, no-xdp, doctor, validate, doctor subsystem).
fn parse_args(args: &[String]) -> Result<(Option<String>, bool, bool, bool, Option<String>)> {
    use clap::Parser;
    let parsed = CliArgs::try_parse_from(args).map_err(|error| anyhow::anyhow!("{error}"))?;
    if parsed.config.is_some() && parsed.positional_config.is_some() {
        anyhow::bail!("multiple config paths given");
    }
    let config_path = parsed.config.or(parsed.positional_config);
    let (doctor, subsystem) = match parsed.command {
        Some(CliCommand::Doctor { subsystem }) => (true, subsystem),
        None => (false, None),
    };
    Ok((
        config_path,
        parsed.no_xdp,
        doctor,
        parsed.validate,
        subsystem,
    ))
}

/// Resolve the operator's config first; repository defaults are a last-resort
/// fallback for source checkouts and development use.
fn doctor_config_path(config_path: Option<String>, doctor: bool) -> Option<String> {
    if !doctor || config_path.is_some() {
        return config_path;
    }
    if let Ok(path) = std::env::var("RAMSHIELD_CONFIG") {
        if !path.trim().is_empty() {
            return Some(path);
        }
    }
    [
        "/etc/ramshield/ramshield.toml",
        "/etc/ramshield/config.toml",
        "config.toml",
        "config.baseline.toml",
    ]
    .iter()
    .find(|path| std::path::Path::new(path).is_file())
    .map(|path| (*path).to_string())
    .or_else(|| Some("/etc/ramshield/ramshield.toml".into()))
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

    let (config_path, no_xdp, doctor, validate, subsystem) = parse_args(&args)?;
    let config_path = doctor_config_path(config_path, doctor);

    // Doctor runs before the normal daemon config path so malformed config is
    // reported as a diagnostic failure rather than aborting before the report.
    if doctor {
        let path = config_path.as_deref().map(std::path::Path::new);
        std::process::exit(ramshield::doctor::run(path, subsystem.as_deref()));
    }

    // --validate: pre-flight hook for systemd `ExecStartPre`. Load + validate
    // the service config, print the verdict, exit 0/1. Does not start the engine.
    if validate {
        let path = match config_path.clone() {
            Some(p) => p,
            None => {
                if std::path::Path::new("/etc/ramshield/config.toml").exists() {
                    "/etc/ramshield/config.toml".into()
                } else {
                    eprintln!(
                        "config validation failed: no config path given and /etc/ramshield/config.toml not found"
                    );
                    std::process::exit(1);
                }
            }
        };
        match Config::load(&path) {
            Ok(cfg) => match cfg.validate() {
                Ok(()) => {
                    println!("config validation passed: {path}");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("config validation failed: {e:#}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("config load failed: {e:#}");
                std::process::exit(1);
            }
        }
    }

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

    // Await pipeline readiness or failure.
    // If boot_pipeline returned Err (WAL corruption, XDP hard fail, etc),
    // this propagates and the process exits with non-zero status.
    engine
        .clone()
        .wait_startup()
        .await
        .context("engine pipeline failed to start")?;

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

#[cfg(test)]
fn command_succeeded(command: &mut std::process::Command) -> bool {
    command.output().is_ok_and(|output| output.status.success())
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
    fn command_succeeded_requires_zero_exit_status() {
        let mut success = std::process::Command::new("sh");
        success.args(["-c", "exit 0"]);
        assert!(super::command_succeeded(&mut success));

        let mut failure = std::process::Command::new("sh");
        failure.args(["-c", "exit 1"]);
        assert!(!super::command_succeeded(&mut failure));
    }

    #[test]
    fn doctor_without_explicit_config_uses_canonical_baseline() {
        assert!(super::doctor_config_path(None, true).is_some());
    }

    #[test]
    fn doctor_preserves_explicit_test_config() {
        assert_eq!(
            super::doctor_config_path(Some("target/test-config.toml".into()), true),
            Some("target/test-config.toml".into())
        );
    }

    #[test]
    fn non_doctor_without_config_keeps_builtin_default_path() {
        assert_eq!(super::doctor_config_path(None, false), None);
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
        let (path, no_xdp, _doctor, _validate, _subsystem) =
            parse_args(&args(&["--no-xdp", "--config", "a.toml"])).unwrap();
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
