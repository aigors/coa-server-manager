//! The HTTP contract: authentication, the request header, the command dispatcher and its error shape.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use coa_web::{router, AppState, Auth};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "test-token-0123456789abcdef0123";

/// The Manager's data folder is one per process (as in the desktop app), so the tests share one state, in a folder
/// with no servers.
fn app(_dir: &tempfile::TempDir) -> axum::Router {
    static SHARED: std::sync::OnceLock<(tempfile::TempDir, std::sync::Arc<AppState>)> = std::sync::OnceLock::new();
    let (_, state) = SHARED.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_path_buf());
        (dir, state)
    });
    router(state.clone(), Auth::token(TOKEN, false), None)
}

fn invoke(cmd: &str) -> axum::http::request::Builder {
    Request::post(format!("/api/invoke/{cmd}")).header(header::CONTENT_TYPE, "application/json")
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, Value, axum::http::HeaderMap) {
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body, headers)
}

#[tokio::test]
async fn health_needs_no_login() {
    let dir = tempfile::tempdir().unwrap();
    let res = app(&dir)
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn commands_need_a_login() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("list_servers")
        .header("x-coa-request", "1")
        .body(Body::from("{}"))
        .unwrap();
    let (status, _, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("list_servers")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, "Bearer nope")
        .body(Body::from("{}"))
        .unwrap();
    let (status, _, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_post_without_the_request_header_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("list_servers")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, _, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_empty_manager_lists_no_servers() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("list_servers")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn an_unknown_server_id_answers_with_the_ui_error_shape() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("server_status")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from(r#"{"id":"nope"}"#))
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["human"]["title"].is_string(), "{body}");
    assert!(body["technical"].is_string(), "{body}");
}

#[tokio::test]
async fn a_command_the_manager_does_not_have_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let req = invoke("no_such_command")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert!(body["technical"].as_str().unwrap().contains("no_such_command"), "{body}");
}

#[tokio::test]
async fn a_desktop_command_reads_its_arguments_by_their_camel_case_names() {
    let dir = tempfile::tempdir().unwrap();
    // list_presets(scope: Scope): a missing argument is the command's own argument error, not "not available".
    let req = invoke("list_presets")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.as_str().unwrap().contains("scope"), "{body}");
    let req = invoke("module_catalog")
        .header("x-coa-request", "1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.as_array().is_some_and(|a| !a.is_empty()), "{body}");
}

#[tokio::test]
async fn login_sets_a_cookie_that_opens_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir);
    let req = Request::post("/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-coa-request", "1")
        .body(Body::from(json!({ "token": TOKEN }).to_string()))
        .unwrap();
    let (status, _, headers) = send(app.clone(), req).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(!cookie.contains(TOKEN), "the cookie must not carry the token itself");
    let pair = cookie.split(';').next().unwrap().to_string();

    let req = invoke("list_servers")
        .header("x-coa-request", "1")
        .header(header::COOKIE, pair)
        .body(Body::from("{}"))
        .unwrap();
    let (status, body, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn info_lists_the_supported_commands() {
    let dir = tempfile::tempdir().unwrap();
    let req = Request::get("/api/info")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, body, _) = send(app(&dir), req).await;
    assert_eq!(status, StatusCode::OK);
    let commands = body["commands"].as_array().unwrap();
    // Every command the desktop app registers, its control-plane module's included.
    for c in ["start_server", "create_backup", "module_settings", "apply_update", "play", "join_realm"] {
        assert!(commands.iter().any(|x| x == c), "{c} missing from {commands:?}");
    }
    assert!(commands.len() > 150, "{}", commands.len());
}
