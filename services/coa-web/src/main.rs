//! `coa-web`: the CoA Server Manager in a browser, for a Linux (Docker) server.
//!
//! Settings (environment):
//! * `COA_WEB_TOKEN`: access token (required). Generate one with `openssl rand -hex 32`.
//! * `COA_WEB_BIND`: listen address, default `127.0.0.1:8787` (put an HTTPS reverse proxy in front of it).
//! * `COA_WEB_STATIC`: folder of the built web interface (`npm run build:web` -> `dist-web`).
//! * `COA_WEB_INSECURE_COOKIE=1`: drop the cookie's `Secure` flag, only for plain-http tests on this machine.
//! * `COA_WEB_NO_AUTH=1`: no token at all; refused unless the address is a loopback address.
//! * `COA_MANAGER_DATA_DIR`, `COA_PACKAGE_SOURCE`, `COA_TRUSTED_KEY`: see `state.rs` and `commands.rs`.

use std::net::SocketAddr;
use std::path::PathBuf;

use coa_web::{router, state, AppState, Auth};

fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind: SocketAddr = std::env::var("COA_WEB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()
        .unwrap_or_else(|e| fail(&format!("COA_WEB_BIND is not an address: {e}")));

    let auth = match std::env::var("COA_WEB_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
        Some(t) if t.trim().len() < 24 => fail("COA_WEB_TOKEN is too short (use at least 24 characters)."),
        Some(t) => Auth::token(t.trim(), !flag("COA_WEB_INSECURE_COOKIE")),
        None if flag("COA_WEB_NO_AUTH") && bind.ip().is_loopback() => {
            tracing::warn!("authentication is OFF (loopback only)");
            Auth::disabled()
        }
        None if flag("COA_WEB_NO_AUTH") => fail("COA_WEB_NO_AUTH is only allowed on a loopback address."),
        None => fail("Set COA_WEB_TOKEN (for example: openssl rand -hex 32)."),
    };

    let static_dir = std::env::var_os("COA_WEB_STATIC").map(PathBuf::from);
    if let Some(d) = &static_dir {
        if !d.join("index.html").is_file() {
            fail(&format!("COA_WEB_STATIC: no index.html in {}", d.display()));
        }
    }

    let data_dir = state::default_data_dir();
    tracing::info!(data = %data_dir.display(), %bind, "coa-web starting");
    let app = router(AppState::new(data_dir), auth, static_dir);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|e| fail(&format!("cannot listen on {bind}: {e}")));
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap_or_else(|e| fail(&e.to_string()));
}

fn fail(message: &str) -> ! {
    eprintln!("coa-web: {message}");
    std::process::exit(2)
}
