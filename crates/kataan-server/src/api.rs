use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticResponse>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticResponse {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultIndex>, ApiError> {
    let vault =
        kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)?;
    Ok(Json(vault.index))
}

pub async fn validate(State(state): State<AppState>) -> Result<Json<ValidateResponse>, ApiError> {
    let report =
        kataan_core::validate::validate(state.vault_path.as_ref()).map_err(ApiError::from)?;
    let ok = report.is_ok();
    let diagnostics = report
        .diagnostics
        .into_iter()
        .map(|diagnostic| DiagnosticResponse {
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            code: diagnostic.code,
            message: diagnostic.message,
            path: diagnostic.path,
        })
        .collect();
    Ok(Json(ValidateResponse { ok, diagnostics }))
}

pub async fn rebuild_indexes(State(state): State<AppState>) -> Result<Json<OkResponse>, ApiError> {
    kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref()).map_err(ApiError::from)?;
    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug)]
pub struct ApiError(anyhow::Error);

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({
            "ok": false,
            "error": self.0.to_string(),
        }));
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn vault_endpoint_returns_root_index() {
        let root = test_vault();
        let app = test_app(&root);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/vault")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn validate_endpoint_returns_diagnostics() {
        let root = test_vault();
        let app = test_app(&root);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/validate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rebuild_indexes_endpoint_repairs_indexes() {
        let root = test_vault();
        let app = test_app(&root);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rebuild-indexes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    fn test_app(root: &Path) -> Router {
        Router::new()
            .route("/api/health", get(health))
            .route("/api/vault", get(vault))
            .route("/api/validate", post(validate))
            .route("/api/rebuild-indexes", post(rebuild_indexes))
            .with_state(AppState::new(root.to_path_buf()))
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
}
