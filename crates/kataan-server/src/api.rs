use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use kataan_core::title::title_from_id;

use crate::{state::AppState, watch::WatchStatus};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub loaded: bool,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct FoldersResponse {
    pub folders: Vec<FolderSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub struct FolderSummaryResponse {
    pub r#type: String,
    pub folder: String,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub document_count: usize,
}

#[derive(Debug, Serialize)]
pub struct FolderResponse {
    pub folder: String,
    pub index: kataan_core::index::FolderIndex,
    pub documents: Vec<FolderDocumentResponse>,
}

#[derive(Debug, Serialize)]
pub struct CanonicalFolderResponse {
    pub id: String,
    pub metadata: Option<kataan_core::document::DocumentMetadata>,
    pub markdown: Option<String>,
    pub folders: Vec<FolderChildResponse>,
    pub documents: Vec<FolderDocumentResponse>,
    pub files: Vec<FolderFileResponse>,
}

#[derive(Debug, Serialize)]
pub struct FolderChildResponse {
    pub id: String,
    pub name: String,
    pub has_index: bool,
}

#[derive(Debug, Serialize)]
pub struct FolderDocumentResponse {
    pub id: String,
    pub slug: String,
    pub markdown: String,
    pub toml: String,
}

#[derive(Debug, Serialize)]
pub struct FolderFileResponse {
    pub name: String,
    pub path: String,
    pub extension: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocumentResponse {
    pub id: String,
    pub type_folder: String,
    pub route_token: String,
    pub metadata: kataan_core::document::DocumentMetadata,
    pub markdown: String,
    pub html: String,
}

#[derive(Debug, Serialize)]
pub struct FileResponse {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct HighlightResponse {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub language: String,
    pub html: String,
}

#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub id: String,
    pub folder: String,
    pub type_folder: String,
    pub route_token: String,
    pub is_folder_index: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticResponse>,
}

#[derive(Debug, Deserialize)]
pub struct IdQuery {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: String,
    pub theme: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub r#type: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticResponse {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

impl From<&kataan_core::diagnostic::Diagnostic> for DiagnosticResponse {
    fn from(diagnostic: &kataan_core::diagnostic::Diagnostic) -> Self {
        Self {
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            path: diagnostic.path.clone(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    // The API owns the `/api` prefix in one place; unknown `/api/*` paths get a
    // 404 from this nested router rather than falling through to the UI shell.
    let api = Router::new()
        .route("/health", get(health))
        .route("/watch", get(watch_status))
        .route("/search", get(search))
        .route("/search/status", get(search_status))
        .route("/search/reindex", post(search_reindex))
        .route("/vault", get(vault))
        .route("/folders", get(folders))
        .route("/folder", get(folder_by_id))
        .route("/document", get(document_by_id))
        .route("/file", get(file_by_path))
        .route("/file/raw", get(raw_file_by_path))
        .route("/file/highlight", get(highlight_file_by_path))
        .route("/resolve", get(resolve_route))
        .route("/schema/:kind", get(schema))
        .route("/folders/:folder", get(folder))
        .route("/documents/*id", get(document))
        .route("/validate", post(validate))
        .route("/rebuild-indexes", post(rebuild_indexes))
        // Own the miss: an unknown /api/* path is a 404, not the SPA shell that
        // the outer fallback would otherwise serve (axum propagates it here).
        .fallback(|| async { StatusCode::NOT_FOUND });

    let app = Router::new().nest("/api", api).with_state(state);

    // With `--features embed-ui`, serve the embedded SPA for any non-API path.
    #[cfg(feature = "embed-ui")]
    let app = app.fallback(crate::ui::serve);

    app
}

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let loaded = state.vault.read().is_ok();
    Json(HealthResponse { ok: true, loaded })
}

pub async fn watch_status(State(state): State<AppState>) -> Json<WatchStatus> {
    let status = state
        .watch
        .read()
        .map(|status| status.clone())
        .unwrap_or_default();
    Json(status)
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<kataan_search::SearchQuery>,
) -> Result<Json<kataan_search::SearchResponse>, ApiError> {
    state
        .search
        .search(&query)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn search_status(
    State(state): State<AppState>,
) -> Result<Json<kataan_search::SearchStatus>, ApiError> {
    kataan_search::SearchIndex::status_for_vault(state.vault_path.as_ref())
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn search_reindex(
    State(state): State<AppState>,
) -> Result<Json<kataan_search::ReindexResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    state
        .search
        .reindex_loaded(&loaded)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultConfig>, ApiError> {
    Ok(Json(read_loaded_vault(&state)?.index.clone()))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let mut folders = Vec::new();

    for (ty, folder) in &loaded.index.type_folders {
        let id = kataan_core::id::CanonicalId::parse(folder)
            .map_err(|source| ApiError::from(anyhow::anyhow!(source)))?;
        let record = loaded.documents.get(&id);
        let document_count = recursive_document_count(&loaded, folder);
        folders.push(FolderSummaryResponse {
            r#type: ty.clone(),
            folder: folder.clone(),
            name: Some(
                record
                    .and_then(document_name)
                    .unwrap_or_else(|| title_from_id(folder)),
            ),
            icon: loaded
                .type_registry
                .definitions
                .get(ty)
                .and_then(|definition| definition.icon.clone()),
            document_count,
        });
    }
    push_code_folder_if_needed(
        &state,
        loaded
            .index
            .type_folders
            .values()
            .any(|folder| kataan_core::constants::is_code_folder(folder)),
        &mut folders,
    );

    Ok(Json(FoldersResponse { folders }))
}

pub async fn folder(
    State(state): State<AppState>,
    Path(folder): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = kataan_core::id::CanonicalId::parse(&folder).map_err(ApiError::bad_request)?;
    let Some(record) = loaded.documents.get(&id) else {
        if kataan_core::constants::is_code_path(id.as_str()) {
            return Ok(Json(FolderResponse {
                folder,
                index: empty_code_folder_index(id.as_str()),
                documents: Vec::new(),
            }));
        }
        return Err(ApiError::not_found(format!(
            "folder `{folder}` does not exist"
        )));
    };
    let documents = direct_documents(&loaded, &id);
    let index = kataan_core::index::FolderIndex {
        name: document_name(record).unwrap_or_else(|| title_from_id(&folder)),
        description: None,
        default_type: Some(record.metadata.r#type.clone()),
        folder_checksum: None,
        documents: Vec::new(),
        subfolders: Vec::new(),
    };

    Ok(Json(FolderResponse {
        folder,
        index,
        documents,
    }))
}

pub async fn folder_by_id(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> Result<Json<CanonicalFolderResponse>, ApiError> {
    canonical_folder_response(&state, &query.id).map(Json)
}

pub async fn document_by_id(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> Result<Json<DocumentResponse>, ApiError> {
    document_response(&state, &query.id).map(Json)
}

pub async fn file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileResponse>, ApiError> {
    file_response(&state, &query.path).map(Json)
}

pub async fn highlight_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<HighlightResponse>, ApiError> {
    highlight_response(&state, &query.path, query.theme.as_deref()).map(Json)
}

pub async fn raw_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<axum::response::Response, ApiError> {
    raw_file_response(&state, &query.path)
}

pub async fn resolve_route(
    State(state): State<AppState>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = loaded
        .resolve_route_token(&query.r#type, &query.token)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "route token `{}` for type `{}` does not resolve",
                query.token, query.r#type
            ))
        })?;
    let document = loaded
        .documents
        .get(&id)
        .ok_or_else(|| ApiError::not_found(format!("document `{id}` does not exist")))?;
    Ok(Json(ResolveResponse {
        id: id.as_str().to_owned(),
        folder: id.containing_folder().to_owned(),
        type_folder: id.top_level_folder().to_owned(),
        route_token: kataan_core::vault::route_token_for_id(&id),
        is_folder_index: document.is_folder_index,
    }))
}

pub async fn document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DocumentResponse>, ApiError> {
    document_response(&state, &id).map(Json)
}

pub async fn schema(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> Result<Json<kataan_core::schema::TomlSchemaResponse>, ApiError> {
    let loaded = read_loaded_vault(&state).ok();
    let response = kataan_core::schema::schema_response(&kind, loaded.as_deref())
        .ok_or_else(|| ApiError::not_found(format!("unknown schema kind `{kind}`")))?;
    Ok(Json(response))
}

pub async fn validate(State(state): State<AppState>) -> Result<Json<ValidateResponse>, ApiError> {
    debug!(vault = %state.vault_path.display(), "validating vault");
    let report =
        kataan_core::validate::validate(state.vault_path.as_ref()).map_err(ApiError::from)?;
    let ok = report.is_ok();
    let diagnostics = report
        .diagnostics
        .iter()
        .map(DiagnosticResponse::from)
        .collect();

    match state.reload() {
        Ok(()) => debug!(vault = %state.vault_path.display(), "reloaded vault after validation"),
        Err(error) => {
            debug!(vault = %state.vault_path.display(), error = %error, "vault reload after validation skipped")
        }
    }

    Ok(Json(ValidateResponse { ok, diagnostics }))
}

pub async fn rebuild_indexes(State(state): State<AppState>) -> Result<Json<OkResponse>, ApiError> {
    info!(vault = %state.vault_path.display(), "rebuilding indexes");
    kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref()).map_err(ApiError::from)?;
    state.reload().map_err(ApiError::from)?;
    info!(vault = %state.vault_path.display(), "reloaded vault after rebuild");
    Ok(Json(OkResponse { ok: true }))
}

/// An API failure carrying the HTTP status it should map to. Handlers build the
/// semantic variants (`not_found`, `bad_request`, `too_large`); any other error
/// converts via `From` and is treated as an internal 500.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: anyhow::Error,
}

impl ApiError {
    fn with_status(status: StatusCode, message: impl std::fmt::Display) -> Self {
        Self {
            status,
            error: anyhow::anyhow!("{message}"),
        }
    }

    pub fn not_found(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::NOT_FOUND, message)
    }

    pub fn bad_request(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::BAD_REQUEST, message)
    }

    pub fn too_large(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::PAYLOAD_TOO_LARGE, message)
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        if self.status.is_server_error() {
            error!(status = %self.status, error = %self.error, "api request failed");
        } else {
            debug!(status = %self.status, error = %self.error, "api request rejected");
        }
        let body = Json(serde_json::json!({
            "ok": false,
            "error": self.error.to_string(),
        }));
        (self.status, body).into_response()
    }
}

mod render;
mod support;
use support::*;

#[cfg(test)]
mod tests;
