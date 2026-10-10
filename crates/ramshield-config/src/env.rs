use super::*;

impl Config {
    /// Apply RAMSHIELD_*__FIELD environment overrides on top of any config.
    ///
    /// Invalid typed values are fatal rather than silently ignored. This is a
    /// security/operations invariant: an operator must never receive a
    /// successful startup while a requested mitigation threshold, memory
    /// limit, connection limit, or feature toggle was rejected by parsing.
    /// Secret values are never included in error messages.
    pub fn apply_env_overrides(&mut self) -> anyhow::Result<()> {
        fn parse<T>(name: &str) -> anyhow::Result<Option<T>>
        where
            T: std::str::FromStr,
            T::Err: std::fmt::Display,
        {
            match std::env::var(name) {
                Ok(value) => value
                    .parse::<T>()
                    .map(Some)
                    .map_err(|e| anyhow::anyhow!("invalid value for {name}: {e}")),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(std::env::VarError::NotUnicode(_)) => {
                    anyhow::bail!("environment variable {name} is not valid UTF-8")
                }
            }
        }

        // Engine overrides
        if let Some(v) = parse::<usize>("RAMSHIELD_ENGINE__RAM_LIMIT_MB")? {
            self.engine.ram_limit_mb = v;
        }
        if let Some(v) = parse::<usize>("RAMSHIELD_ENGINE__WORKER_THREADS")? {
            self.engine.worker_threads = v;
        }
        if let Some(v) = parse::<usize>("RAMSHIELD_ENGINE__SHARD_COUNT")? {
            self.engine.shard_count = v
                .checked_next_power_of_two()
                .ok_or_else(|| anyhow::anyhow!("RAMSHIELD_ENGINE__SHARD_COUNT is too large"))?;
        }

        // Detection overrides
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__RPS_THRESHOLD")? {
            self.detection.rps_threshold = v;
        }
        if let Some(v) = parse::<u32>("RAMSHIELD_DETECTION__PROMOTE_MIN_EVENTS")? {
            self.detection.promote_min_events = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__BATCH_WINDOW_MS")? {
            self.detection.batch_window_ms = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__SUBNET_WINDOW_THRESHOLD")? {
            self.detection.subnet_window_threshold = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__BLOCK_TTL_SECS")? {
            self.detection.block_ttl_secs = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__SUBNET_BURST_TTL_SECS")? {
            self.detection.subnet_burst_ttl_secs = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__RATE_WINDOW_SECS")? {
            self.detection.rate_window_secs = v;
        }
        if let Some(v) = parse::<usize>("RAMSHIELD_DETECTION__SUBNET_BATCH_THRESHOLD")? {
            self.detection.subnet_batch_threshold = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__SUBNET_BATCH_MIN_EVENTS")? {
            self.detection.subnet_batch_min_events = v;
        }
        if let Some(v) = parse::<bool>("RAMSHIELD_DETECTION__BATCH_BLOCK_ENABLED")? {
            self.detection.batch_block_enabled = v;
        }

        // L7 / application-cost detection overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_DETECTION__L7_ENABLED")? {
            self.detection.l7_enabled = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__L7_RPS_THRESHOLD")? {
            self.detection.l7_rps_threshold = v;
        }
        if let Some(v) = parse::<u32>("RAMSHIELD_DETECTION__L7_HTTP2_MIN_STREAMS")? {
            self.detection.l7_http2_min_streams = v;
        }
        if let Some(v) = parse::<u8>("RAMSHIELD_DETECTION__L7_HTTP2_RESET_RATIO_PCT")? {
            self.detection.l7_http2_reset_ratio_pct = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_DETECTION__L7_BLOCK_TTL_SECS")? {
            self.detection.l7_block_ttl_secs = v;
        }

        // Mesh overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_MESH__ENABLED")? {
            self.mesh.enabled = v;
        }
        if let Some(v) = parse::<u32>("RAMSHIELD_MESH__NODE_ID")? {
            self.mesh.node_id = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_MESH__LISTEN_ADDR") {
            self.mesh.listen_addr = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_MESH__PEERS") {
            self.mesh.peers = v
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned)
                .collect();
        }
        if let Ok(v) = std::env::var("RAMSHIELD_MESH__AUTH_KEY") {
            self.mesh.auth_key = v;
        }

        // Autonomous/native/SYNPROXY overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_AUTONOMOUS__ENABLED")? {
            self.autonomous.enabled = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_AUTONOMOUS__SYN_PPS_PER_CPU")? {
            self.autonomous.syn_pps_per_cpu = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_AUTONOMOUS__UDP_PPS_PER_CPU")? {
            self.autonomous.udp_pps_per_cpu = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_AUTONOMOUS__PACKET_PPS_PER_CPU")? {
            self.autonomous.packet_pps_per_cpu = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_AUTONOMOUS__WINDOW_MS")? {
            self.autonomous.window_ms = v;
        }
        if let Some(v) = parse::<bool>("RAMSHIELD_NATIVE_INGEST__ENABLED")? {
            self.native_ingest.enabled = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_NATIVE_INGEST__INTERFACE") {
            self.native_ingest.interface = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_NATIVE_INGEST__MAX_EVENTS_PER_SEC")? {
            self.native_ingest.max_events_per_sec = v;
        }
        if let Some(v) = parse::<bool>("RAMSHIELD_SYNPROXY__ENABLED")? {
            self.synproxy.enabled = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_SYNPROXY__PORTS") {
            self.synproxy.ports = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        }

        // Upstream saturation/BGP escalation overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_UPSTREAM__ENABLED")? {
            self.upstream.enabled = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__INTERFACE") {
            self.upstream.interface = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_UPSTREAM__LINK_CAPACITY_MBPS")? {
            self.upstream.link_capacity_mbps = v;
        }
        if let Some(v) = parse::<u8>("RAMSHIELD_UPSTREAM__SATURATION_PCT")? {
            self.upstream.saturation_pct = v;
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_UPSTREAM__POLL_MS")? {
            self.upstream.poll_ms = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__WEBHOOK_URL") {
            self.upstream.webhook_url = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Some(v) = parse::<u64>("RAMSHIELD_UPSTREAM__COOLDOWN_SECS")? {
            self.upstream.cooldown_secs = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__BGP_MODE") {
            self.upstream.bgp_mode = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__BGP_FIFO") {
            self.upstream.bgp_fifo = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__PROTECTED_PREFIX") {
            self.upstream.protected_prefix = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("RAMSHIELD_UPSTREAM__BGP_COMMUNITY") {
            self.upstream.bgp_community = if v.trim().is_empty() { None } else { Some(v) };
        }

        // IPC overrides
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__AUTH_KEYS") {
            self.ipc.auth_keys = v
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__TCP_ADDR") {
            self.ipc.tcp_addr = v;
        }
        if let Some(v) = parse::<usize>("RAMSHIELD_IPC__MAX_CONNECTIONS")? {
            self.ipc.max_connections = v;
        }

        // Dashboard overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_DASHBOARD__ENABLED")? {
            self.dashboard.enabled = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__HTTP_ADDR") {
            self.dashboard.http_addr = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__ADMIN_PASSWORD") {
            use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
            let salt = SaltString::generate(&mut OsRng);
            self.dashboard.admin_password_hash = Some(
                argon2::Argon2::default()
                    .hash_password(v.as_bytes(), &salt)
                    .map_err(|e| {
                        anyhow::anyhow!("failed to hash RAMSHIELD_DASHBOARD__ADMIN_PASSWORD: {e}")
                    })?
                    .to_string(),
            );
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__ADMIN_PASSWORD_HASH") {
            let v = v.trim().to_string();
            if !v.is_empty() {
                self.dashboard.admin_password_hash = Some(v);
            }
        }

        // Forecasting overrides
        if let Some(v) = parse::<bool>("RAMSHIELD_FORECASTING__ENABLED")? {
            self.forecasting.enabled = v;
        }

        Ok(())
    }
}
