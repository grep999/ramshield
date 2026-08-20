use ramshield::config::Config;
use ramshield::forecasting::Forecaster;
use ramshield::storage::Store;
use ramshield_types::command::Command;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_forecaster_triggers_block() {
    let config = Config::default();
    let store = Arc::new(Store::new(16));
    let (tx, mut rx) = mpsc::channel(100);
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let learner = Arc::new(ramshield::learning::PatternLearner::new(store.clone(), config.detection.clone(), tx.clone()));
    
    let forecaster = Arc::new(Forecaster::new(
        store.clone(),
        config.forecasting.clone(),
        tx,
        metrics,
        learner,
    ));

    tokio::spawn(forecaster.clone().run());

    // Simulate a traffic spike
    store.traffic.events_last_second.store(5000, std::sync::atomic::Ordering::Relaxed);
    store.traffic.unique_ips_window.store(50, std::sync::atomic::Ordering::Relaxed);
    
    // Give the forecaster time to run
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Check if a block command was sent
    let cmd = rx.try_recv();
    assert!(cmd.is_ok(), "Forecaster should have sent a block command");
    
    match cmd.unwrap() {
        Command::Enforcement(e) => {
            if let ramshield_types::command::EnforcementCommand::Block(b) = e {
                assert_eq!(b.reason, ramshield_types::BlockReason::ForecastAnomaly);
            } else {
                panic!("Expected a block command");
            }
        }
        _ => panic!("Expected an enforcement command"),
    }
}
