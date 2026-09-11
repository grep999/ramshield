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
use tracing::warn;

const COOKIE_NAME: &str = "rs_session";

#[derive(Clone)]
pub struct AuthState {
    /// Argon2 PHC string from config.
    password_hash: Option<String>,
    ttl: Duration,
    sessions: Arc<DashMap<String, Instant, ahash::RandomState>>,
    max_login_attempts: u32,
    max_password_length: usize,
    /// Per-IP failed-login counters. A global counter let any host lock out
    /// every admin with 50 garbage POSTs (process-wide DoS). Windowed per IP:
    /// failures older than LOCKOUT_WINDOW decay and the slot is reclaimed.
    failures: Arc<DashMap<IpAddr, FailureWindow, ahash::RandomState>>,
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

impl AuthState {
    pub fn new(
        password_hash: Option<String>,
        ttl_secs: u64,
        max_login_attempts: u32,
        max_password_length: usize,
    ) -> Self {
        // P3 fix: an unparseable PHC hash made verify_password() return
        // None forever — indistinguishable from a wrong password, i.e. a
        // silently un-loginable dashboard. Fail loudly at startup instead.
        if let Some(h) = password_hash.as_deref()
            && argon2::PasswordHash::new(h).is_err()
        {
            tracing::error!(
                "dashboard.admin_password_hash is not a valid PHC string — logins WILL fail until fixed"
            );
        }
        Self {
            password_hash,
            ttl: Duration::from_secs(ttl_secs.max(60)),
            sessions: Arc::new(DashMap::with_hasher(ahash::RandomState::new())),
            max_login_attempts,
            max_password_length,
            failures: Arc::new(DashMap::with_hasher(ahash::RandomState::new())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.password_hash.is_some()
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
        let hash = self.password_hash.as_ref()?;
        let parsed = argon2::PasswordHash::new(hash).ok()?;
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
        // Opportunistic sweep of expired sessions (sharded, no global lock).
        self.sessions.retain(|_, t| t.elapsed() < self.ttl);
        self.sessions.contains_key(token)
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
    if path == "/healthz" || path == "/login" || path.starts_with("/static/") {
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
    Html(include_str!("login.html").replace("{{ERR}}", "")).into_response()
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(
    State(auth): State<AuthState>,
    addr: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<LoginForm>,
) -> Response {
    // Per-IP lockout: one hostile host can no longer lock every admin out.
    // Option extractor: absent ConnectInfo (unit tests) falls back to ::,
    // which still rate-limits the un-identified path.
    let ip = addr.map(|c| c.0.ip()).unwrap_or(IpAddr::from([0, 0, 0, 0]));
    if auth.is_locked(ip) {
        warn!(
            "dashboard login locked out from {ip} ({}+ failures)",
            auth.max_login_attempts
        );
        return (StatusCode::TOO_MANY_REQUESTS, "locked").into_response();
    }
    // Argon2 verify burns ~50-100 ms of CPU. Inline on an async handler it
    // blocks the Tokio worker — 20 concurrent bad logins stall every route
    // on those workers. Run it on the blocking pool.
    let blocking_auth = auth.clone();
    let password = form.password.clone();
    let verified = tokio::task::spawn_blocking(move || blocking_auth.verify_password(&password))
        .await
        .unwrap_or(None);
    match verified {
        Some(token) => {
            auth.register_session(&token);
            let cookie = format!(
                "{}={}; HttpOnly; SameSite=Lax; Secure; Path=/; Max-Age={}",
                COOKIE_NAME,
                token,
                auth.ttl.as_secs()
            );
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
            let page = include_str!("login.html")
                .replace("{{ERR}}", "<p class=\"err\">Invalid credentials.</p>");
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
        let a = AuthState::new(Some(hash_of("hunter2")), 3600, 50, 1024);
        assert!(a.enabled());
        assert!(login(&a, "wrong").is_none());
        let tok = login(&a, "hunter2").expect("good pw logs in");
        assert!(a.validate(&tok));
        assert!(!a.validate("deadbeef"));
    }

    #[test]
    fn disabled_auth_has_no_sessions() {
        let a = AuthState::new(None, 3600, 50, 1024);
        assert!(!a.enabled());
        assert!(login(&a, "x").is_none()); // no hash → nothing validates
    }

    #[test]
    fn lockout_is_per_ip_not_global() {
        let a = AuthState::new(Some(hash_of("hunter2")), 3600, 3, 1024);
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
}
