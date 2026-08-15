//! Serve the web UI (apps/web/dist) embedded into the binary.
//!
//! The web app is a client-routed SPA, so any path the API didn't handle falls
//! back to index.html. The dev server enforces the same rule (see the
//! spa-fallback integration in apps/web/astro.config.mjs).

use std::borrow::Cow;

use axum::{
    body::{Body, Bytes},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// The static web build (apps/web/dist), embedded at compile time.
#[derive(RustEmbed)]
#[folder = "../../apps/web/dist/"]
struct WebAssets;

/// Serve an embedded asset, or the SPA shell (index.html) for deep links.
pub async fn serve(uri: Uri) -> Response {
    let lookup = uri.path().trim_start_matches('/');
    let lookup = if lookup.is_empty() {
        "index.html"
    } else {
        lookup
    };

    let direct = WebAssets::get(lookup);
    // Only cache a content-hashed asset that actually exists; a miss that falls
    // back to the SPA shell must not be cached under the requested asset URL.
    let cacheable = direct.is_some() && lookup.starts_with("_astro/");
    let Some(asset) = direct.or_else(|| WebAssets::get("index.html")) else {
        return (
            StatusCode::NOT_FOUND,
            "kataan web UI is not embedded; build with `--features embed-ui`",
        )
            .into_response();
    };

    // Assets are baked into the binary (Cow::Borrowed, 'static) in release
    // builds, so serve them without copying; only debug builds, which read dist
    // from disk, own their bytes.
    let body = match asset.data {
        Cow::Borrowed(bytes) => Body::from(Bytes::from_static(bytes)),
        Cow::Owned(bytes) => Body::from(bytes),
    };
    let mut response = ([(header::CONTENT_TYPE, asset.metadata.mimetype())], body).into_response();

    // Content-hashed /_astro files can be cached forever (see `cacheable`).
    if cacheable {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}
