//! HTTP-level tests, split by what a route does.
//!
//! The fixtures live here because every group builds the same test vault.

mod query;
mod reads;
mod writes;

use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use tower::ServiceExt;

use super::render::*;
use super::support::*;
use super::*;

async fn json_response<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Send a JSON body. Write routes take one; `request` does not.
async fn json_request(
    app: Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

/// As `json_request`, but claiming to come from another site.
async fn cross_site_request(
    app: Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("sec-fetch-site", "cross-site")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn request(app: Router, method: &str, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

fn test_app(root: &Path) -> Router {
    router(AppState::new(root.to_path_buf()).unwrap())
}

fn test_vault() -> PathBuf {
    let root = unique_temp_dir();
    kataan_core::init::init_vault(&root, "Test Vault").unwrap();
    root
}

fn unique_temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "kataan-server-test-{}-{counter}",
        std::process::id()
    ))
}
