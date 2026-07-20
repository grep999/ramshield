use ramshield::install_panic_hook;

fn main() -> Result<()> {
    // Atomic P0: --version flag (BACKLOG #8) — checked before tracing init
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ramshield {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ramshield=info")),
        )
        .init();

    let mut config_path: Option<String> = None;
    let mut test_scenario_name: Option<String> = None;

    // Parse CLI arguments
    let mut i = 1; // Start from 1 to skip program name
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            config_path = Some(args[i + 1].clone());
            i += 2; // Consume both --config and its value
        } else if args[i] == "--run-tests" && i + 1 < args.len() {
            test_scenario_name = Some(args[i + 1].clone());
            i += 2; // Consume both --run-tests and its value
        } else {
            // Unrecognized argument, or argument without value
            // We can add error handling here or just ignore
            i += 1;
        }
    }

    let config = match config_path {
        Some(path) => {
            let absolute_path = std::fs::canonicalize(&path).map_err(|e| anyhow::anyhow!("Error canonicalizing path {}: {}", path, e))?;
            eprintln!("Attempting to load config from absolute path: {:?}", absolute_path);
            Config::load(absolute_path.to_str().unwrap())?
        },
        None       => Config::default(),
    };

    if let Some(test_name) = test_scenario_name {
        // Run tests
        let test_scenarios: Vec<TestScenario> = serde_json::from_str(
            &fs::read_to_string("rs/feature_tests.json")?
        )?;
        let scenario = test_scenarios.into_iter().find(|s| s.name == test_name)
            .ok_or_else(|| anyhow::anyhow!("Test scenario not found: {}", test_name))?;

        run_test_scenario(scenario, config).await?;
        info!("Test scenario finished.");
        Ok(())
    } else {
        // Start RamShield normally (original main logic)
        let engine = Arc::new(Engine::new(config.clone()));
        engine.start();

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
}

async fn run_test_scenario(scenario: TestScenario, config: Config) -> Result<()> {
    info!("Running test scenario: {}", scenario.name);

    // Start RamShield in background
    let engine = Arc::new(Engine::new(config.clone()));
    engine.start();

    // Give RamShield time to start up
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Execute attack command
    let cmd_parts = shlex::split(&scenario.command)
        .ok_or_else(|| anyhow::anyhow!("Invalid command: {}", scenario.command))?;
    
    let python_path = "/usr/bin/python3"; // Explicit path
    let project_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let full_script_path = format!("{}/rs/scripts/{}", project_root.display(), scenario.script);

    eprintln!("Executing: {} {} with args {:?}", python_path, full_script_path, cmd_parts);

    let output = Command::new(python_path)
        .arg(&full_script_path)
        .args(&cmd_parts)
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("Attack command failed: {:?}\nStdout: {}\nStderr: {}", output, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }

    // Monitor API
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await; // Give time for processing

    // ** Dashboard check is removed from here for now **
    // The curl command will fail if dashboard is not started, so it's not useful to check for it yet.
    // Instead, we will directly check the engine's internal state.

    // Get snapshot directly from engine (needs API endpoint for this or direct access)
    // For now, assume curl works IF dashboard runs.
    // So if curl failed above, this test would bail.
    // We assume dashboard is not failing due to configuration
    // (test_config.toml dashboard.enabled = true) but rather internal error.
    
    // So, this is still needed to check ips_tracked:
    let snapshot_output = Command::new("/usr/bin/curl")
        .arg("http://127.0.0.1:7891/api/snapshot")
        .output()
        .await?;

    if !snapshot_output.status.success() {
        anyhow::bail!("Failed to get API snapshot: {:?}\nStdout: {}\nStderr: {}", snapshot_output, String::from_utf8_lossy(&snapshot_output.stdout), String::from_utf8_lossy(&snapshot_output.stderr));
    }
    let snapshot: HashMap<String, serde_json::Value> = serde_json::from_slice(&snapshot_output.stdout)?;

    for check in scenario.monitor_api {
        if let Some(value) = snapshot.get(&check.field) {
            // Basic condition check - needs more robust parsing for actual comparisons
            info!("API check: {} {} -> Current value: {}", check.field, check.condition, value);
            
            let current_value = value.as_f64().unwrap_or_default();
            let mut condition_parts = shlex::split(&check.condition)
                .ok_or_else(|| anyhow::anyhow!("Invalid condition: {}", check.condition))?;
            
            if condition_parts.len() != 2 {
                anyhow::bail!("Invalid condition format, expected operator and value: {}", check.condition);
            }

            let operator = condition_parts.remove(0);
            let target_value = condition_parts.remove(0).parse::<f64>()?;

            let passed = match operator.as_str() {
                ">" => current_value > target_value,
                "<" => current_value < target_value,
                ">=" => current_value >= target_value,
                "<=" => current_value <= target_value,
                "==" => current_value == target_value,
                _ => anyhow::bail!("Unsupported operator: {}", operator),
            };

            if !passed {
                 anyhow::bail!("API check failed for {}: {} {} (current: {})", check.field, check.condition, target_value, current_value);
            }

        } else {
            anyhow::bail!("API field not found in snapshot: {}", check.field);
        }
    }

    // Check logs (placeholder for now, needs log capturing and regex matching)
    for expected_log in scenario.expected_logs {
        info!("Checking for log pattern: {}", expected_log);
        // This needs actual log capturing from ramshield background process
    }

    // Initiate graceful shutdown (similar to main's shutdown logic)
    engine.shutdown();
    let shutdown_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    while engine.is_shutting_down() && tokio::time::Instant::now() < shutdown_deadline {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Ok(())
}

