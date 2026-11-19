//! Web backend for the CoA Server Manager (prototype).
//!
//! The Manager's React interface talks to its backend through Tauri's `invoke(command, args)` and `listen(event)`. This
//! service answers the same calls over HTTP, so the unchanged interface can run in a browser and manage a Linux (Docker)
//! server on another machine:
//!
//! * `POST /api/invoke/{command}` with the JSON arguments: the command's result as JSON, or a `UiError` (the same error
//!   shape the desktop app sends) with a 4xx status.
//! * `GET /api/events`: Server-Sent Events, one `{event, payload}` message per event (for example `install-progress`).
//! * `POST /api/login` / `POST /api/logout`: an access token exchanged for an `HttpOnly`, `SameSite=Strict` cookie.
//!
//! As in the desktop app, every mutating command takes an installation id and resolves the path from the registry.

pub mod commands;
pub mod state;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tower_http::services::{ServeDir, ServeFile};

pub use state::AppState;

const COOKIE: &str = "coa_session";
/// Every call that changes something must carry this header. A page on another site cannot add it without a CORS
/// preflight, which this service never allows, so a forged form or link cannot act with the visitor's cookie.
const REQUEST_HEADER: &str = "x-coa-request";

/// How the service checks who is calling.
#[derive(Clone)]
pub struct Auth {
    /// SHA-256 of the access token; `None` only when authentication was switched off for a loopback-only address.
    token_hash: Option<[u8; 32]>,
    /// Mark the session cookie `Secure` (keep it on behind an HTTPS reverse proxy).
    secure_cookie: bool,
}

impl Auth {
    pub fn token(token: &str, secure_cookie: bool) -> Self {
        Self {
            token_hash: Some(hash(token)),
            secure_cookie,
        }
    }

    pub fn disabled() -> Self {
        Self {
            token_hash: None,
            secure_cookie: false,
        }
    }

    fn accepts(&self, presented: &str) -> bool {
        match &self.token_hash {
            None => true,
            // Comparing digests of equal length leaks nothing useful about the token through timing.
            Some(expected) => hash(presented) == *expected,
        }
    }

    /// The cookie value is derived from the token, never the token itself.
    fn session_value(&self) -> String {
        match &self.token_hash {
            Some(h) => hex::encode(hash(&format!("coa-web-session:{}", hex::encode(h)))),
            None => "open".into(),
        }
    }
}

fn hash(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

pub struct Web {
    pub app: Arc<AppState>,
    pub auth: Auth,
}

/// The whole service. `static_dir` is the built web interface (`npm run build:web`), served for every other path.
pub fn router(app: Arc<AppState>, auth: Auth, static_dir: Option<PathBuf>) -> Router {
    let web = Arc::new(Web { app, auth });
    let api = Router::new()
        .route("/api/info", get(info))
        .route("/api/invoke/{command}", post(invoke))
        .route("/api/events", get(events))
        .route_layer(middleware::from_fn_with_state(web.clone(), require_auth));
    let open = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout));
    let mut router = api.merge(open).with_state(web);
    if let Some(dir) = static_dir {
        let index = dir.join("index.html");
        router = router.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    router
}

fn cookie_session(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn require_auth(State(web): State<Arc<Web>>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let authorised = web.auth.token_hash.is_none()
        || bearer(headers).is_some_and(|t| web.auth.accepts(t))
        || cookie_session(headers).is_some_and(|c| hash(&c) == hash(&web.auth.session_value()));
    if !authorised {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "login required" }))).into_response();
    }
    if req.method() != Method::GET && !headers.contains_key(REQUEST_HEADER) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": format!("missing {REQUEST_HEADER} header") })),
        )
            .into_response();
    }
    next.run(req).await
}

#[derive(Deserialize)]
struct Login {
    token: String,
}

async fn login(State(web): State<Arc<Web>>, headers: HeaderMap, Json(body): Json<Login>) -> Response {
    if !headers.contains_key(REQUEST_HEADER) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !web.auth.accepts(&body.token) {
        // Slows down guessing; the token is long and random, so this is only a second line.
        tokio::time::sleep(Duration::from_millis(700)).await;
        tracing::warn!("rejected login");
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "wrong token" }))).into_response();
    }
    let secure = if web.auth.secure_cookie { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{secure}",
        web.auth.session_value()
    );
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

async fn logout() -> Response {
    let cookie = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

async fn info() -> Json<Value> {
    Json(json!({
        "name": "CoA Server Manager (web)",
        "version": env!("CARGO_PKG_VERSION"),
        "flavor": coa_core::platform::flavor(),
        "commands": commands::SUPPORTED,
    }))
}

async fn invoke(
    State(web): State<Arc<Web>>,
    Path(command): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    let args = if args.is_null() { json!({}) } else { args };
    match commands::dispatch(&web.app, &command, args).await {
        Ok(v) => Json(v).into_response(),
        Err(commands::CallError::Unknown) => {
            let e: coa_core::error::UiError = coa_core::Error::Invalid(format!(
                "\"{command}\" is not available in the web version yet."
            ))
            .into();
            (StatusCode::NOT_IMPLEMENTED, Json(e)).into_response()
        }
        Err(commands::CallError::Ui(e)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(e)).into_response(),
    }
}

async fn events(State(web): State<Arc<Web>>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(web.app.events.subscribe()).filter_map(|m| {
        // A client too slow to keep up misses events (`Lagged`); the screens poll the state they show anyway.
        let m = m.ok()?;
        Some(Ok(Event::default().data(serde_json::to_string(&m).ok()?)))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}
