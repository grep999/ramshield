use anyhow::Result;
use ramshield::{dashboard, Config, Engine};
use std::sync::Arc;
use tracing::{info, debug}; // Add debug
use tracing_subscriber::EnvFilter;

#[cfg(feature = "otel")]
fn init_otel() -> opentelemetry_sdk::trace::TracerProvider {
    opentelemetry_sdk::trace::TracerProvider::default()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Atomic P0: --version flag (BACKLOG #8) — checked before tracing init
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ramshield {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ramshield=info"));

    #[cfg(feature = "otel")]
    {
        use opentelemetry::trace::TracerProvider;
        let provider = init_otel();
        let _tracer = provider.tracer("ramshield");
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
        let _ = (_tracer, provider);
    }
    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let mut config_path: Option<String> = None;

    // Parse CLI arguments
    let mut i = 1; // Start from 1 to skip program name
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            config_path = Some(args[i + 1].clone());
            i += 2; // Consume both --config and its value
        } else {
            // Unrecognized argument, or argument without value
            i += 1;
        }
    }

    let config = match config_path {
        Some(path) => {
            let absolute_path = std::fs::canonicalize(&path)
                .map_err(|e| anyhow::anyhow!("Error canonicalizing path {}: {}", path, e))?;
            eprintln!(
                "Attempting to load config from absolute path: {:?}",
                absolute_path
            );
            Config::load(absolute_path.to_str().unwrap())?
        }
        None => Config::default(),
    };
    info!("Loaded config: {:#?}", config);
    debug!("Loaded config: {:#?}", config);

    // Start RamShield normally
    let store = Arc::new(ramshield::storage::Store::new(config.engine.shard_count));
    store.traffic.ram_limit_mb.store(config.engine.ram_limit_mb, std::sync::atomic::Ordering::Relaxed);
    // Store created_at for uptime tracking
    store.traffic.uptime_secs.store(1, std::sync::atomic::Ordering::Relaxed); // mark non-zero
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(config.clone(), store.clone(), metrics.clone()));
    let _engine_handle = engine.clone().start_async().expect("engine pipeline");

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
    if dashboard_config.enabled {
        std::thread::Builder::new()
            .name("rs-dashboard".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .expect("dashboard runtime");
                rt.block_on(async move {
                    if let Err(e) = dashboard::serve(eng_clone, &dashboard_config.http_addr).await {
                        tracing::error!("Dashboard server error: {}", e);
                    }
                });
            })
            .expect("spawn dashboard thread");
    }

    info!("RamShield running — Ctrl+C to stop");

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await?;

    // Initiate graceful shutdown
    engine.shutdown();

    // Give batch processor time to drain (5 seconds max)
    let shutdown_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    while engine.is_shutting_down() && tokio::time::Instant::now() < shutdown_deadline {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    info!("Shutdown complete.");
    Ok(())
}
