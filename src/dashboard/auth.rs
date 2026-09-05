//! Dashboard session auth. Single admin, Argon2 password, in-process session
//! store. Enabled by `[dashboard] admin_password_hash`; without it the
//! dashboard stays open (dev/loopback default).
use axum::{
    Form, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
};
use rand::RngCore;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::warn;

const COOKIE_NAME: &str = "rs_session";

#[derive(Clone)]
pub struct AuthState {
    /// Argon2 PHC string from config.
    password_hash: Option<String>,
    ttl: Duration,
    sessions: Arc<std::sync::Mutex<HashMap<String, Instant>>>,
    max_login_attempts: u32,
    max_password_length: usize,
    /// Process-wide failed-login counter. thread_local was a no-op under
    /// tokio (each worker sees 0); one AtomicU32 covers every request.
    failed_logins: Arc<AtomicU32>,
}

impl AuthState {
    pub fn new(
        password_hash: Option<String>,
        ttl_secs: u64,
        max_login_attempts: u32,
        max_password_length: usize,
    ) -> Self {
        Self {
            password_hash,
            ttl: Duration::from_secs(ttl_secs.max(60)),
            sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
            max_login_attempts,
            max_password_length,
            failed_logins: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.password_hash.is_some()
    }

    fn login(&self, password: &str) -> Option<String> {
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
        let token = hex::encode(token);
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(token.clone(), Instant::now());
        Some(token)
    }

    fn validate(&self, token: &str) -> bool {
        if token.len() != 64 {
            return false;
        }
        // ponytail: poison-recovery — session map data stays valid across a
        // panicked holder; availability > strict poison semantics.
        let mut map = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Opportunistic sweep of expired sessions.
        map.retain(|_, t| t.elapsed() < self.ttl);
        map.contains_key(token)
    }

    fn failed_logins(&self) -> u32 {
        // ponytail: process-wide counter, resets on restart. Swap for a
        // per-IP limiter when the dashboard faces hostile networks.
        self.failed_logins.load(Ordering::Relaxed)
    }

    fn note_failure(&self) {
        self.failed_logins.fetch_add(1, Ordering::Relaxed);
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

async fn login_page(State(auth): State<AuthState>) -> Html<&'static str> {
    if !auth.enabled() {
        return Html(
            "<html><body><p>Auth disabled — set [dashboard] admin_password_hash.</p></body></html>",
        );
    }
    Html(include_str!("login.html"))
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(
    State(auth): State<AuthState>,
    Form(form): Form<LoginForm>,
) -> Response {
    if auth.failed_logins() >= auth.max_login_attempts {
        warn!(
            "dashboard login locked out ({}+ failures)",
            auth.max_login_attempts
        );
        return (StatusCode::TOO_MANY_REQUESTS, "locked").into_response();
    }
    match auth.login(&form.password) {
        Some(token) => {
            let cookie = format!(
                "{}={}; HttpOnly; SameSite=Lax; Max-Age={}",
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
            auth.note_failure();
            (
                StatusCode::UNAUTHORIZED,
                Html("<html><body><p>wrong password</p></body></html>"),
            )
                .into_response()
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

    #[test]
    fn login_sets_session_and_validates() {
        let a = AuthState::new(Some(hash_of("hunter2")), 3600, 50, 1024);
        assert!(a.enabled());
        assert!(a.login("wrong").is_none());
        let tok = a.login("hunter2").expect("good pw logs in");
        assert!(a.validate(&tok));
        assert!(!a.validate("deadbeef"));
    }

    #[test]
    fn disabled_auth_has_no_sessions() {
        let a = AuthState::new(None, 3600, 50, 1024);
        assert!(!a.enabled());
        assert!(a.login("x").is_none()); // no hash → nothing validates
    }

    #[test]
    fn lockout_is_process_wide_not_thread_local() {
        let a = AuthState::new(Some(hash_of("hunter2")), 3600, 3, 1024);
        for _ in 0..4 {
            assert!(a.login("wrong").is_none());
            a.note_failure();
        }
        assert!(a.failed_logins() >= a.max_login_attempts);
        // Same AuthState cloned across "threads" still sees the counter.
        let b = a.clone();
        assert!(b.failed_logins() >= b.max_login_attempts);
    }
}
