use super::*;
use serial_test::serial;

#[test]
fn pre_aggregation_map_size_has_hard_ceiling() {
    let mut cfg = Config::default();
    cfg.detection.pre_aggs_max_size = 1_000_000;
    cfg.validate().expect("documented ceiling must be accepted");

    cfg.detection.pre_aggs_max_size = 1_000_001;
    let err = cfg.validate().expect_err("size above ceiling must be rejected");
    assert!(err.to_string().contains("pre_aggs_max_size"), "{err}");

    cfg.detection.pre_aggs_max_size = usize::MAX;
    let err = cfg
        .validate()
        .expect_err("effectively unbounded pre-aggregation must be rejected");
    assert!(err.to_string().contains("pre_aggs_max_size"), "{err}");
}

#[test]
fn forecasting_seasonality_has_hard_allocation_ceiling() {
    let mut cfg = Config::default();
    cfg.forecasting.enabled = true;
    cfg.forecasting.seasonality_period = 86_400;
    cfg.validate().expect("one-day seasonal vector must be accepted");

    cfg.forecasting.seasonality_period = 86_401;
    let err = cfg
        .validate()
        .expect_err("seasonal vector above one day must be rejected");
    assert!(err.to_string().contains("seasonality_period"), "{err}");

    cfg.forecasting.seasonality_period = usize::MAX;
    let err = cfg
        .validate()
        .expect_err("effectively unbounded seasonal vector must be rejected");
    assert!(err.to_string().contains("seasonality_period"), "{err}");
}

#[test]
fn wal_volatile_fallback_defaults_false() {
    assert!(!WalConfig::default().allow_volatile_fallback);
    let parsed: WalConfig = toml::from_str(
        r#"
enabled = true
dir = "/tmp/w"
durability = "Flush"
compress = true
seg_max_bytes = 1
retention_max_bytes = 1
"#,
    )
    .expect("partial wal without allow_volatile_fallback");
    assert!(!parsed.allow_volatile_fallback);
}

/// IPv6 plan Task 6: bracketed v6 binds are the documented form
/// ("[::]:7890"); the public-exposure guard must see through the
/// brackets or an unauthenticated dashboard binds all-interfaces
/// while validate() thinks it's loopback-only.
#[test]
fn public_bind_covers_v6_forms() {
    assert!(is_public_bind("[::]:7890"), "bracketed any-v6");
    assert!(
        is_public_bind(":::7890"),
        "unbracketed any-v6 host '::' + port"
    );
    assert!(is_public_bind("[*]:7890"), "bracketed star");
    assert!(is_public_bind("[0.0.0.0]:80"), "bracketed v4-any");
    assert!(
        !is_public_bind("[::1]:7890"),
        "bracketed loopback is private"
    );
    assert!(
        !is_public_bind("::1:7890"),
        "unbracketed loopback is private"
    );
    assert!(
        !is_public_bind("[fe80::1]:7890"),
        "bracketed v6 link-local is private"
    );
    assert!(
        !is_public_bind("127.0.0.1:7890"),
        "v4 loopback stays private"
    );
}

/// P1: NIC-specific binds reach the LAN. An unauthenticated dashboard on
/// 192.168.x.x is one ARP hop from every host on the segment — the
/// fail-closed guard must not treat it as operator-local. Hostnames stay
/// private (localhost resolves loopback; a DNS name needs an answer we
/// can't get here). Link-local is unreachable from other hosts.
#[test]
fn lan_nic_binds_are_public() {
    assert!(is_public_bind("192.168.1.5:9999"), "v4 private LAN");
    assert!(is_public_bind("10.0.0.1:7890"), "v4 private 10/8");
    assert!(is_public_bind("172.16.0.1:7890"), "v4 private 172.16/12");
    assert!(is_public_bind(":9999"), "empty host binds 0.0.0.0");
    assert!(!is_public_bind("[fe80::1]:7890"), "v6 link-local private");
    // ponytail: 'localhost' (RFC 6761) is the only hostname exempted from
    // fail-closed. All other unparseable hostnames are treated as public
    // to prevent shipping a passwordless dashboard on a DNS name.
    assert!(
        !is_public_bind("localhost:9999"),
        "localhost is RFC 6761 loopback"
    );
    assert!(
        is_public_bind("dashboard.example.com:9999"),
        "arbitrary hostname is fail-closed public"
    );
}

/// Vuln 3 (Secure cookie lockout on non-loopback HTTP): the loopback
/// classifier is the mirror of is_public_bind but stricter — it only
/// accepts 127.0.0.0/8 and ::1. Browsers accept `Secure` cookies over
/// plain HTTP only for loopback origins (RFC 6265bis §5.3 "trustworthy
/// origin"); a non-loopback bind without TLS silently drops the cookie.
#[test]
fn loopback_bind_only_accepts_loopback() {
    assert!(is_loopback_bind("127.0.0.1:9999"), "v4 loopback");
    assert!(is_loopback_bind("127.0.0.0:9999"), "v4 loopback net base");
    assert!(
        is_loopback_bind("127.255.255.255:9999"),
        "v4 loopback net top"
    );
    assert!(is_loopback_bind("[::1]:9999"), "v6 loopback bracketed");
    assert!(is_loopback_bind("::1:9999"), "v6 loopback unbracketed");
    assert!(!is_loopback_bind("192.168.1.5:9999"), "LAN is non-loopback");
    assert!(
        !is_loopback_bind("10.0.0.1:7890"),
        "private is non-loopback"
    );
    assert!(!is_loopback_bind("0.0.0.0:9999"), "any-v4 is non-loopback");
    assert!(!is_loopback_bind("[::]:9999"), "any-v6 is non-loopback");
    assert!(!is_loopback_bind(":9999"), "empty host is non-loopback");
    assert!(
        !is_loopback_bind("localhost:9999"),
        "hostname is non-loopback"
    );
}

/// Vuln 3: exposure_warnings must fire the Secure-cookie warning for a
/// non-loopback bind without TLS, and must stay silent for loopback
/// (the default 127.0.0.1:9999) and for tls_enabled=true.
#[test]
fn secure_cookie_warning_fires_off_loopback() {
    let mut c = Config::default();
    // Default: loopback, no TLS → the loopback-Secure warning does not fire.
    let secure_warn = |w: &str| w.contains("Secure session cookie");
    assert!(
        !c.exposure_warnings().iter().any(|w| secure_warn(w)),
        "loopback must not fire the Secure-cookie HTTP warning"
    );
    // LAN bind without TLS → the loopback-Secure warning fires.
    c.dashboard.http_addr = "192.168.1.50:9999".into();
    assert!(
        c.exposure_warnings().iter().any(|w| secure_warn(w)),
        "LAN bind without TLS must fire the Secure-cookie HTTP warning"
    );
    // Same bind with TLS fronting → the loopback-Secure warning silenced.
    c.dashboard.tls_enabled = true;
    assert!(
        !c.exposure_warnings().iter().any(|w| secure_warn(w)),
        "tls_enabled=true must silence the Secure-cookie HTTP warning"
    );
}

#[test]
fn duplicate_ipc_key_ids_are_rejected() {
    let mut cfg = Config::default();
    cfg.ipc.auth_keys = vec![
        "k1:0102030405060708090a0b0c0d0e0f10".into(),
        "k1:1112131415161718191a1b1c1d1e1f20".into(),
    ];
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("duplicate key_id"), "{err}");
}

#[test]
fn resource_limits_are_bounded() {
    let mut cfg = Config::default();
    cfg.engine.worker_threads = 257;
    assert!(cfg.validate().is_err());
    cfg.engine.worker_threads = 1;
    cfg.ipc.max_connections = 8193;
    assert!(cfg.validate().is_err());
}

#[test]
fn relative_detection_validation_is_finite_and_representable() {
    let cfg = Config::default();
    assert!(!cfg.detection.relative_enabled);
    assert_eq!(cfg.detection.relative_min_samples, 8);
    assert_eq!(cfg.detection.relative_min_breaches, 3);
    cfg.validate().unwrap();

    let mut bad = Config::default();
    bad.detection.relative_factor = f64::NAN;
    assert!(bad.validate().is_err());
    bad.detection.relative_factor = f64::INFINITY;
    assert!(bad.validate().is_err());
    bad.detection.relative_factor = 1.0;
    bad.detection.relative_floor_rps = f64::NAN;
    assert!(bad.validate().is_err());
    bad.detection.relative_floor_rps = 1.0;
    bad.detection.relative_min_samples = 0;
    assert!(bad.validate().is_err());
    bad.detection.relative_min_samples = 256;
    assert!(bad.validate().is_err());
    bad.detection.relative_min_samples = 8;
    bad.detection.relative_min_breaches = 0;
    assert!(bad.validate().is_err());
}

#[test]
fn forecasting_validation_rejects_non_finite_and_out_of_range_values() {
    let mut cfg = Config::default();
    cfg.forecasting.enabled = true;

    cfg.forecasting.hw_beta = f64::NAN;
    assert!(cfg.validate().is_err());
    cfg.forecasting.hw_beta = 0.1;

    cfg.forecasting.hw_gamma = f64::INFINITY;
    assert!(cfg.validate().is_err());
    cfg.forecasting.hw_gamma = 0.1;

    cfg.forecasting.anomaly_zscore = f64::NAN;
    assert!(cfg.validate().is_err());
    cfg.forecasting.anomaly_zscore = 2.5;

    cfg.forecasting.min_entropy = f64::NEG_INFINITY;
    assert!(cfg.validate().is_err());
    cfg.forecasting.min_entropy = 2.0;

    cfg.forecasting.hw_beta = 1.1;
    assert!(cfg.validate().is_err());
    cfg.forecasting.hw_beta = 0.1;
    cfg.forecasting.hw_gamma = -0.1;
    assert!(cfg.validate().is_err());
}

#[cfg(test)]
fn clear_env_vars() {
    let keys = [
        "RAMSHIELD_ENGINE__RAM_LIMIT_MB",
        "RAMSHIELD_ENGINE__WORKER_THREADS",
        "RAMSHIELD_ENGINE__SHARD_COUNT",
        "RAMSHIELD_DETECTION__RPS_THRESHOLD",
        "RAMSHIELD_DETECTION__PROMOTE_MIN_EVENTS",
        "RAMSHIELD_DETECTION__BATCH_WINDOW_MS",
        "RAMSHIELD_DETECTION__SUBNET_WINDOW_THRESHOLD",
        "RAMSHIELD_DETECTION__BLOCK_TTL_SECS",
        "RAMSHIELD_IPC__TCP_ADDR",
        "RAMSHIELD_IPC__MAX_CONNECTIONS",
        "RAMSHIELD_DASHBOARD__ENABLED",
        "RAMSHIELD_DASHBOARD__HTTP_ADDR",
        "RAMSHIELD_FORECASTING__ENABLED",
    ];
    for k in &keys {
        unsafe {
            std::env::remove_var(k);
        }
    }
}

#[test]
fn default_config_validates() {
    let cfg = Config::default();
    cfg.validate().unwrap();
    assert!(
        cfg.exposure_warnings().is_empty(),
        "loopback-only config must warn about nothing"
    );
}

#[test]
fn public_binds_produce_exposure_warnings() {
    let mut cfg = Config::default();
    cfg.dashboard.http_addr = "0.0.0.0:9999".into();
    cfg.dashboard.admin_password_hash = Some("$argon2id$v=19$m=19456,t=2,p=1$rOGcgxnibWHynWZ0exEH7Q$KM06+4aIAIc2nPNe+jyGekH+zqzAwwYw3JHzgo26b1M".into());
    cfg.ipc.tcp_addr = "0.0.0.0:7890".into();
    cfg.ipc.auth_keys =
        vec!["k1:0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021".into()];
    cfg.ipc.behind_tls_proxy = true;
    cfg.dashboard.tls_enabled = true;
    // validate() permits public binds only with explicit TLS/auth assertions;
    // warnings still make the deployment boundary visible.
    cfg.validate().unwrap();
    let w = cfg.exposure_warnings();
    assert!(
        w.iter().any(|s| s.contains("dashboard.http_addr")),
        "expected dashboard warning, got {w:?}"
    );
    assert!(
        w.iter().any(|s| s.contains("ipc.tcp_addr")),
        "expected ipc warning, got {w:?}"
    );
}

#[test]
fn public_dashboard_without_password_is_rejected() {
    let mut cfg = Config::default();
    cfg.dashboard.http_addr = "0.0.0.0:9999".into();
    cfg.dashboard.admin_password_hash = None;
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("admin_password_hash"), "{err}");
}

#[test]
fn public_ipc_without_keys_is_rejected() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "0.0.0.0:7890".into();
    cfg.ipc.auth_keys.clear();
    cfg.ipc.behind_tls_proxy = true;
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("auth_keys"), "{err}");
}

#[test]
fn public_ipc_without_tls_proxy_is_rejected() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "0.0.0.0:7890".into();
    cfg.ipc.auth_keys =
        vec!["k1:0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021".into()];
    cfg.ipc.behind_tls_proxy = false;
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("behind_tls_proxy"), "{err}");
}

#[test]
fn public_ipc_with_tls_proxy_and_keys_validates() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "0.0.0.0:7890".into();
    cfg.ipc.auth_keys =
        vec!["k1:0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021".into()];
    cfg.ipc.behind_tls_proxy = true;
    cfg.validate().unwrap();
}

#[test]
fn loopback_without_auth_still_validates() {
    let cfg = Config::default();
    assert_eq!(cfg.dashboard.http_addr, "127.0.0.1:9999");
    assert_eq!(cfg.ipc.tcp_addr, "127.0.0.1:7890");
    cfg.validate().unwrap();
}

#[test]
fn require_auth_on_loopback_without_keys_is_rejected() {
    let mut cfg = Config::default();
    cfg.ipc.require_auth = true;
    cfg.ipc.auth_keys.clear();
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("require_auth"), "{err}");
}

#[test]
fn auth_key_without_key_id_is_rejected() {
    // A bare hex blob (no `:`) or `:hex` passes the length/hex checks below
    // it, but parse_ipc_keys rejects the entry at bind time — which used to
    // leave the listener up with ZERO active keys (silent downgrade to
    // unauthenticated). validate() must fail first.
    let hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021";
    let mut cfg = Config::default();

    cfg.ipc.auth_keys = vec![hex.into()];
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("key_id"), "{err}");

    cfg.ipc.auth_keys = vec![format!(":{hex}")];
    let err = cfg.validate().unwrap_err().to_string();
    assert!(err.contains("key_id"), "{err}");

    // Sanity: the same secret WITH a key_id is accepted.
    cfg.ipc.auth_keys = vec![format!("k1:{hex}")];
    cfg.validate().unwrap();
}

#[test]
fn require_auth_with_valid_key_validates() {
    let mut cfg = Config::default();
    cfg.ipc.require_auth = true;
    cfg.ipc.auth_keys =
        vec!["k1:0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021".into()];
    cfg.validate().unwrap();
}

#[test]
fn subnet_burst_ttl_default_is_short_and_serde_defaults_apply() {
    // Regression: subnet batch blocks used to inherit block_ttl_secs (1h),
    // locking out whole /24s of shared egress for an hour.
    let cfg = Config::default();
    assert_eq!(cfg.detection.subnet_burst_ttl_secs, 120);
    assert!(cfg.detection.subnet_burst_ttl_secs < cfg.detection.block_ttl_secs);
    // Old TOML without the field must still parse (serde default) —
    // parse just the [detection] table; other tables have their own requireds.
    let parsed: DetectionConfig = toml::from_str(
        "rps_threshold = 100\nrate_window_secs = 10\nsubnet_batch_threshold = 50\nsubnet_batch_min_events = 100\nbatch_block_enabled = true\nblock_ttl_secs = 3600\nbloom_bits = 1000",
    )
    .unwrap();
    assert_eq!(parsed.subnet_burst_ttl_secs, 120);
}

#[test]
#[serial]
fn env_var_override_ram_limit() {
    clear_env_vars();
    unsafe {
        std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "1024");
    }
    let tmpfile = "/tmp/ramshield_test_config.toml";
    std::fs::write(tmpfile, "").unwrap();
    let cfg = Config::load(tmpfile).unwrap();
    assert_eq!(cfg.engine.ram_limit_mb, 1024);
    clear_env_vars();
}

#[test]
#[serial]
fn env_override_detection_rps() {
    clear_env_vars();
    unsafe {
        std::env::set_var("RAMSHIELD_DETECTION__RPS_THRESHOLD", "500");
    }
    let tmpfile = "/tmp/ramshield_test_config.toml";
    std::fs::write(tmpfile, "").unwrap();
    let cfg = Config::load(tmpfile).unwrap();
    assert_eq!(cfg.detection.rps_threshold, 500);
    clear_env_vars();
}

/// P1 regression: env override must not smuggle a public bind past the
/// fail-closed validation that only ran on the file. Before the fix,
/// Config::load validated the file, applied env overrides, and discarded
/// the re-validation result — so this env combo booted an open server.
#[test]
#[serial]
fn env_override_public_bind_is_rejected() {
    clear_env_vars();
    unsafe {
        std::env::set_var("RAMSHIELD_IPC__TCP_ADDR", "0.0.0.0:7890");
    }
    let tmpfile = "/tmp/ramshield_test_config.toml";
    std::fs::write(tmpfile, "").unwrap();
    let err =
        Config::load(tmpfile).expect_err("public IPC bind without auth_keys must fail startup");
    assert!(err.to_string().contains("auth_keys"), "{err}");
    clear_env_vars();
}

#[test]
#[serial]
fn env_override_invalid_value_is_rejected() {
    clear_env_vars();
    unsafe {
        std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "not_a_number");
    }
    let tmpfile = "/tmp/ramshield_test_config.toml";
    std::fs::write(tmpfile, "").unwrap();
    let err = Config::load(tmpfile).expect_err("invalid typed env override must fail startup");
    assert!(
        err.to_string().contains("RAMSHIELD_ENGINE__RAM_LIMIT_MB"),
        "{err}"
    );
    clear_env_vars();
}
