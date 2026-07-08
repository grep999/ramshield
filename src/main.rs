use anyhow::{Context, Result};
use ramshield::{Config, Engine};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ramshield=info")),
        )
        .init();

    info!("RamShield starting up...");

    // Load config from file path argument, or fall back to default
    let config = {
        let args: Vec<String> = std::env::args().collect();
        if args.len() >= 2 {
            let path = &args[1];
            info!("Loading config from: {}", path);
            Config::load_from_file(path)
                .map_err(|e| anyhow::anyhow!("Failed to load config from {}: {}", path, e))?
        } else {
            info!("No config file specified, using default config");
            let c = Config::default();
            c.validate().context("Configuration validation failed")?;
            c
        }
    };

    let engine = Arc::new(Engine::new(config));
    engine.start().await?;

    info!("RamShield running — Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for Ctrl+C")?;

    info!("Ctrl+C received, initiating graceful shutdown...");
    engine.shutdown().await?;

    info!("Shutdown complete.");
    Ok(())
}