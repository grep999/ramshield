//! Dashboard session auth. Single admin, Argon2 password, in-process session
//! store. Enabled by `[dashboard] admin_password_hash`; without it the
//! dashboard stays open (dev/loopback default).
use axum::{
    Form, Router,
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
};
use dashmap::DashMap;
use rand::RngCore;
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tracing::warn;

const COOKIE_NAME: &str = "rs_session";

/// Build the Set-Cookie value for a session token. Pure so unit tests can
/// assert HttpOnly / SameSite / Secure / Path / Max-Age without the full
/// login handler.
pub(crate) fn session_cookie_value(token: &str, ttl_secs: u64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax{secure_flag}; Path=/; Max-Age={ttl_secs}"
    )
}

#[derive(Clone)]
pub struct AuthState {
    /// Argon2 PHC string from config. RwLock so a hot-reloaded config can
    /// swap it without restarting the dashboard server (D4 audit fix).
    password_hash: Arc<std::sync::RwLock<Option<String>>>,
    ttl: Duration,
    sessions: Arc<DashMap<String, Instant, ahash::RandomState>>,
    max_login_attempts: u32,
    max_password_length: usize,
    /// Reverse proxies trusted to send `X-Forwarded-For` (CWE-307 fix).
    /// Empty = trust no proxy; lockout key = direct TCP peer address.
    trusted_proxies: Arc<Vec<String>>,
    /// Per-IP failed-login counters. A global counter let any host lock out
    /// every admin with 50 garbage POSTs (process-wide DoS). Windowed per IP:
    /// failures older than LOCKOUT_WINDOW decay and the slot is reclaimed.
    failures: Arc<DashMap<IpAddr, FailureWindow, ahash::RandomState>>,
    /// Last time the session store was swept. Used by validate() to throttle
    /// the O(n) retain to once per SWEEP_INTERVAL instead of every request.
    last_sweep: Arc<std::sync::atomic::AtomicI64>,
    /// Whether to stamp the Secure flag on session cookies. Browsers drop
    /// Secure cookies on non-trustworthy origins (non-loopback plain HTTP)
    /// which silently breaks login. ponytail: true by default; serve() sets
    /// it based on the bind address at startup.
    secure_cookie: bool,
    /// Metrics handle for dashboard auth counters.
    pub(crate) metrics: Arc<ramshield_metrics::Metrics>,
    /// Max concurrent Argon2 hash operations (default 4). Prevents a flood of
    /// bad logins from saturating the blocking pool with 100ms CPU-bound hashes.
    /// 0 = unlimited (original behavior).
    argon2_parallelism: u32,
    /// Semaphore to limit Argon2 concurrent operations.
    argon2_semaphore: Arc<Semaphore>,
}

/// Rolling failure window for one client IP.
#[derive(Clone)]
struct FailureWindow {
    count: u32,
    first_fail: Instant,
}

/// Failed attempts older than this decay to zero — transient brute force
/// stops locking the IP after a cool-down instead of until restart.
const LOCKOUT_WINDOW: Duration = Duration::from_secs(15 * 60);

/// HTML-escape user-controlled strings before template substitution (SEC-10,
/// reflected XSS). Only markup-significant chars need escaping; everything
/// passes through unchanged.
fn sanitize_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            _ => escaped.push(c),
        }
    }
    escaped
}
/// Sweep interval for the session store. Rather than scanning all
/// sessions on every authenticated request (O(n)), we sweep at most
/// once per SWEEP_INTERVAL. Entries that outlive the TTL are still
/// reclaimed lazily on access; the sweep is only to bound the map size.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

impl AuthState {
    // ponytail: too_many_arguments suppressed intentionally — builder pattern
    // adds ceremony without clarity for a constructor called once at startup.
    // collapsible_if is suppressed to keep the PHC validation below readable
    // instead of folding it into a let-chain.
    #[allow(clippy::too_many_arguments, clippy::collapsible_if)]
    pub fn new(
        password_hash: Option<String>,
        ttl_secs: u64,
        max_login_attempts: u32,
        max_password_length: usize,
        trusted_proxies: Vec<String>,
        secure_cookie: bool,
        metrics: Arc<ramshield_metrics::Metrics>,
        argon2_parallelism: u32,
    ) -> Self {
        // P3 fix: an unparseable PHC hash made verify_password() return
        // None forever — indistinguishable from a wrong password, i.e. a
        // silently un-loginable dashboard. Fail loudly at startup instead.
        if let Some(h) = password_hash.as_deref() {
            if argon2::PasswordHash::new(h).is_err() {
                tracing::error!(
                    "dashboard.admin_password_hash is not a valid PHC string — logins WILL fail until fixed"
                );
            }
        }
        let parallelism = argon2_parallelism.max(1);
        // Production floors at 60s. Tests allow 1s to prove expiry without long sleeps.
        #[cfg(not(test))]
        let min_ttl = 60u64;
        #[cfg(test)]
        let min_ttl = 1u64;
        Self {
            password_hash: Arc::new(std::sync::RwLock::new(password_hash)),
            ttl: Duration::from_secs(ttl_secs.max(min_ttl)),
            sessions: Arc::new(DashMap::with_hasher(ahash::RandomState::new())),
            max_login_attempts,
            max_password_length,
            trusted_proxies: Arc::new(trusted_proxies),
            failures: Arc::new(DashMap::with_hasher(ahash::RandomState::new())),
            last_sweep: Arc::new(std::sync::atomic::AtomicI64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            )),
            secure_cookie,
            metrics,
            argon2_parallelism: parallelism,
            argon2_semaphore: Arc::new(Semaphore::new(parallelism as usize)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.password_hash
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(true) // Poisoned state must never disable authentication.
    }

    /// Hot-swap the password hash without restarting the dashboard.
    /// Callers must have already validated the new PHC string.
    ///
    /// A poisoned lock remains fail-closed until this explicit replacement.
    /// The replacement is written while holding the recovered write guard,
    /// then poison is cleared so readers can resume with the known-good value.
    pub fn set_password_hash(&self, new_hash: Option<String>) {
        match self.password_hash.write() {
            Ok(mut guard) => *guard = new_hash,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = new_hash;
                self.password_hash.clear_poison();
            }
        }
    }

    /// True when this IP is currently locked out. Entries for IPs whose
    /// window has fully decayed are removed here (opportunistic sweep —
    /// the map is bounded by distinct IPs that actually POST /login).
    fn is_locked(&self, ip: IpAddr) -> bool {
        let expired = match self.failures.get(&ip) {
            None => return false,
            Some(e) => e.first_fail.elapsed() >= LOCKOUT_WINDOW,
        };
        if expired {
            self.failures.remove(&ip);
            return false;
        }
        self.failures
            .get(&ip)
            .is_some_and(|e| e.count >= self.max_login_attempts)
    }

    fn note_failure(&self, ip: IpAddr) {
        // P3 fix: entries were only reclaimed when the SAME ip returned —
        // a many-source (IPv6-rotating) bad-password flood grew the map
        // without bound. Cheap cap: sweep expired windows past 10k entries.
        if self.failures.len() > 10_000 {
            self.failures
                .retain(|_, w| w.first_fail.elapsed() < LOCKOUT_WINDOW);
        }
        self.failures
            .entry(ip)
            .and_modify(|w| {
                if w.first_fail.elapsed() >= LOCKOUT_WINDOW {
                    // Window expired — restart it with this failure.
                    w.count = 1;
                    w.first_fail = Instant::now();
                } else {
                    w.count += 1;
                }
            })
            .or_insert(FailureWindow {
                count: 1,
                first_fail: Instant::now(),
            });
    }

    /// Pure password verification — no shared state, safe to run on a
    /// blocking thread. Argon2 verify burns ~50-100ms of CPU; calling it
    /// inline on an async handler blocks the Tokio worker for every other
    /// request on that thread.
    fn verify_password(&self, password: &str) -> Option<String> {
        // A poisoned lock means the hash may have been left in an unknown
        // state. Deny login until set_password_hash installs a known-good hash.
        let hash = self.password_hash.read().ok()?.as_ref()?.clone();
        let parsed = argon2::PasswordHash::new(&hash).ok()?;
        // Constant-time verify inside argon2; cap work on garbage input.
        if password.len() > self.max_password_length {
            return None;
        }
        let ok = {
            use argon2::password_hash::PasswordVerifier;
            argon2::Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        };
        if !ok {
            return None;
        }
        let mut token = [0u8; 32];
        rand::rng().fill_bytes(&mut token);
        Some(hex::encode(token))
    }

    /// Record a verified token as an active session. Split out of the login
    /// flow so the async handler can verify on a blocking thread and register
    /// here.
    fn register_session(&self, token: &str) {
        self.sessions.insert(token.to_string(), Instant::now());
    }

    fn validate(&self, token: &str) -> bool {
        if token.len() != 64 {
            return false;
        }
        // ponytail: throttle the O(n) retain sweep to once per SWEEP_INTERVAL
        // (memory bound only). Expiry itself is enforced exactly below via a
        // per-token TTL check — a throttled sweep alone would let an expired
        // session authenticate for up to SWEEP_INTERVAL past its TTL.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let last = self.last_sweep.load(std::sync::atomic::Ordering::Relaxed);
        if now - last >= SWEEP_INTERVAL.as_secs() as i64 {
            self.sessions.retain(|_, t| t.elapsed() < self.ttl);
            self.last_sweep
                .store(now, std::sync::atomic::Ordering::Relaxed);
        }
        self.sessions
            .get(token)
            .is_some_and(|t| t.elapsed() < self.ttl)
    }
}

/// Middleware: gate every request unless auth disabled or path exempted.
pub async fn require_auth(
    State(state): State<crate::dashboard::AppState>,
    req: Request,
    next: Next,
) -> Response {
    let auth = &state.auth;
    if !auth.enabled() {
        return next.run(req).await;
    }
    let path = req.uri().path();
    // ponytail: /static/ is dead — nothing serves it (the HUD is inlined via
    // include_str! in mod.rs). Leaving the prefix exemption is a latent
    // unauthenticated surface the moment a static mount is added. Drop it.
    if path == "/healthz" || path == "/livez" || path == "/login" || path == "/metrics" {
        return next.run(req).await;
    }
    let valid = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix(COOKIE_NAME)
                    .and_then(|rest| rest.strip_prefix('='))
            })
        })
        .map(|tok| auth.validate(tok))
        .unwrap_or(false);
    if valid {
        return next.run(req).await;
    }
    if path.starts_with("/api/") {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error":"unauthorized"})),
        )
            .into_response()
    } else {
        // ponytail: build() can only fail on invalid header values. The
        // inputs here (SEE_OTHER status code, "/login" path) are constants
        // http::HeaderValue always accepts — this match is belt-and-suspenders.
        // Upgrade to axum::response::Redirect when the target becomes
        // user-supplied; the typed builder eliminates the runtime path entirely.
        match Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, "/login")
            .body(axum::body::Body::empty())
        {
            Ok(r) => r,
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "redirect builder failed (hard-coded inputs — should be unreachable)",
            )
                .into_response(),
        }
    }
}

async fn login_page(State(auth): State<AuthState>) -> Response {
    if !auth.enabled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "<html><body><p>Auth disabled — set [dashboard] admin_password_hash.</p></body></html>",
        )
            .into_response();
    }
    Html(include_str!("login.html").replace("{{ERR}}", "")).into_response() // static "" — no user input
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(
    State(auth): State<AuthState>,
    addr: Option<ConnectInfo<SocketAddr>>,
    headers: axum::http::header::HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    // Per-IP lockout: one hostile host can no longer lock every admin out.
    // Option extractor: absent ConnectInfo (unit tests) falls back to ::,
    // which still rate-limits the un-identified path.
    //
    // CWE-307 fix: behind a trusted reverse proxy, extract the real client IP
    // from X-Forwarded-For. If the peer is NOT a trusted proxy, fall back to
    // the direct TCP peer address so a shared proxy IP doesn't collapse every
    // admin into one lockout bucket.
    let ip = if let Some(c) = addr {
        let peer = c.0.ip();
        if ramshield_config::peer_is_trusted_proxy(peer, &auth.trusted_proxies) {
            // Trusted proxy — use X-Forwarded-For client IP (first entry = original client)
            let xff_name = axum::http::header::HeaderName::from_static("x-forwarded-for");
            let xff = headers.get(&xff_name).and_then(|v| v.to_str().ok());
            ramshield_config::xff_client(xff).unwrap_or(peer)
        } else {
            // Untrusted peer — key by direct TCP address only
            peer
        }
    } else {
        // No ConnectInfo — unit test path, use placeholder
        IpAddr::from([0, 0, 0, 0])
    };
    if auth.is_locked(ip) {
        warn!(
            "dashboard login locked out from {ip} ({}+ failures)",
            auth.max_login_attempts
        );
        auth.metrics.inc_auth_lockout();
        return (StatusCode::TOO_MANY_REQUESTS, "locked").into_response();
    }
    // Argon2 verify burns ~50-100 ms of CPU. Inline on an async handler it
    // blocks the Tokio worker — 20 concurrent bad logins stall every route
    // on those workers. Run it on the blocking pool, bounded by semaphore.
    let argon2_permit = if auth.argon2_parallelism > 0 {
        // Await acquisition — yield the worker while waiting. This is safe
        // because the Semaphore never blocks a worker for the full hash
        // time; it only gates admission to spawn_blocking.
        let p = auth.argon2_semaphore.acquire().await;
        p.ok()
    } else {
        None
    };
    // Qual metric: time the Argon2 verify to surface overload (auth_verification_wait_ms).
    let verify_start = Instant::now();
    let blocking_auth = auth.clone();
    let password = form.password.clone();
    let verified = tokio::task::spawn_blocking(move || {
        let _ = argon2_permit; // hold permit through blocking work
        blocking_auth.verify_password(&password)
    })
    .await
    .unwrap_or(None);
    let elapsed = verify_start.elapsed();
    auth.metrics
        .set_auth_verification_wait_ms(elapsed.as_millis() as u64);
    match verified {
        Some(token) => {
            auth.register_session(&token);
            let cookie = session_cookie_value(&token, auth.ttl.as_secs(), auth.secure_cookie);
            // ponytail: cookie value is a hex token (header::HeaderValue::from_str
            // accepts it); attrs are ASCII constants. The match is
            // belt-and-suspenders. If the cookie ever embeds user-supplied bytes,
            // swap to typed Set-Cookie helpers in axum-extra.
            match Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header(header::SET_COOKIE, cookie)
                .header(header::LOCATION, "/")
                .body(axum::body::Body::empty())
            {
                Ok(r) => r,
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "login response builder failed (controlled inputs — should be unreachable)",
                )
                    .into_response(),
            }
        }
        None => {
            auth.note_failure(ip);
            // Same page, inline error — no context-switch to a bare HTML stub.
            // SEC-10: error text passes through sanitize_html before template
            // substitution — static today, safe if the message ever becomes
            // caller-influenced (lockout reason, IP echo, etc).
            let page = include_str!("login.html").replace(
                "{{ERR}}",
                &format!(
                    r#"<p class="err">{}</p>"#,
                    sanitize_html("Invalid credentials.")
                ),
            );
            (StatusCode::UNAUTHORIZED, Html(page)).into_response()
        }
    }
}

pub fn router() -> Router<AuthState> {
    Router::new().route("/login", get(login_page).post(login_submit))
}

pub use require_auth as middleware;

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(pw: &str) -> String {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
        let salt = SaltString::generate(&mut OsRng);
        argon2::Argon2::default()
            .hash_password(pw.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    /// Helper mirroring what login_submit does: verify on (here: inline),
    /// then register.
    fn login(a: &AuthState, pw: &str) -> Option<String> {
        let tok = a.verify_password(pw)?;
        a.register_session(&tok);
        Some(tok)
    }

    #[test]
    fn login_sets_session_and_validates() {
        let a = AuthState::new(
            Some(hash_of("hunter2")),
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        assert!(a.enabled());
        assert!(login(&a, "wrong").is_none());
        let tok = login(&a, "hunter2").expect("good pw logs in");
        assert!(a.validate(&tok));
        assert!(!a.validate("deadbeef"));
    }

    #[test]
    fn disabled_auth_has_no_sessions() {
        let a = AuthState::new(
            None,
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        assert!(!a.enabled());
        assert!(login(&a, "x").is_none()); // no hash → nothing validates
    }

    #[test]
    fn lockout_is_per_ip_not_global() {
        let a = AuthState::new(
            Some(hash_of("hunter2")),
            3600,
            3,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        let attacker = IpAddr::from([1, 2, 3, 4]);
        let admin = IpAddr::from([5, 6, 7, 8]);
        for _ in 0..4 {
            a.note_failure(attacker);
        }
        assert!(a.is_locked(attacker));
        // Attacker burning attempts must NOT lock out a different IP.
        assert!(!a.is_locked(admin));
        // Same Arc-shared state seen through a clone.
        let b = a.clone();
        assert!(b.is_locked(attacker));
    }

    #[test]
    fn session_expires_after_ttl() {
        let a = AuthState::new(
            Some(hash_of("hunter2")),
            1,
            50,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        let tok = login(&a, "hunter2").expect("login");
        assert!(a.validate(&tok));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!a.validate(&tok), "session must expire after TTL");
    }

    #[test]
    fn cookie_flags_http_only_samesite_secure_and_path() {
        let c = session_cookie_value("abc123", 3600, true);
        assert!(c.contains("HttpOnly"), "{c}");
        assert!(c.contains("SameSite=Lax"), "{c}");
        assert!(c.contains("Secure"), "{c}");
        assert!(c.contains("Path=/"), "{c}");
        assert!(c.contains("Max-Age=3600"), "{c}");
        assert!(c.starts_with("rs_session=abc123"), "{c}");
        let plain = session_cookie_value("tok", 60, false);
        assert!(!plain.contains("Secure"), "{plain}");
        assert!(
            plain.contains("HttpOnly") && plain.contains("SameSite=Lax"),
            "{plain}"
        );
    }

    #[test]
    fn login_lockout_blocks_after_max_attempts() {
        let a = AuthState::new(
            Some(hash_of("hunter2")),
            3600,
            3,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        let ip = IpAddr::from([9, 9, 9, 9]);
        for _ in 0..3 {
            a.note_failure(ip);
        }
        assert!(a.is_locked(ip));
        assert!(!a.is_locked(IpAddr::from([8, 8, 8, 8])));
    }

    #[test]
    fn invalid_phc_hash_never_authenticates() {
        let a = AuthState::new(
            Some("not-a-valid-phc-hash".into()),
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        assert!(a.enabled());
        assert!(login(&a, "anything").is_none());
    }

    #[test]
    fn poisoned_auth_fails_closed_until_valid_hash_replacement() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let a = AuthState::new(
            Some(hash_of("old-password")),
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );

        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = a.password_hash.write().unwrap();
            panic!("simulate panic while mutating auth state");
        }));

        assert!(a.enabled(), "poisoned state must keep auth enabled");
        assert!(
            a.verify_password("old-password").is_none(),
            "poisoned state must deny login"
        );

        a.set_password_hash(Some(hash_of("new-password")));
        assert!(a.enabled());
        assert!(a.verify_password("old-password").is_none());
        assert!(
            a.verify_password("new-password").is_some(),
            "explicit replacement should clear poison and restore login"
        );
    }

    #[test]
    fn oversized_password_is_rejected() {
        let a = AuthState::new(
            Some(hash_of("hunter2")),
            3600,
            50,
            16,
            vec![],
            true,
            Arc::new(ramshield_metrics::Metrics::new()),
            4,
        );
        assert!(a.verify_password(&"x".repeat(32)).is_none());
    }

    #[test]
    fn sanitize_html_neutralizes_markup() {
        let s = sanitize_html(r#"<script>alert(1)</script>&""#);
        assert!(!s.contains('<') && !s.contains('>'), "{s}");
        assert!(s.contains("&amp;"), "{s}");
    }

    #[test]
    fn deployment_defaults_are_loopback_with_finite_ttl_and_lockout() {
        let d = ramshield_config::DashboardConfig::default();
        assert!(
            d.http_addr.starts_with("127.0.0.1"),
            "default bind must be loopback: {}",
            d.http_addr
        );
        assert_eq!(d.session_ttl_secs, 28_800);
        assert_eq!(d.max_login_attempts, 50);
        assert!(d.admin_password_hash.is_none());
        assert_eq!(d.cookie_secure, None);
    }
}
