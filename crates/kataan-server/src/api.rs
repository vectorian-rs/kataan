use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

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
pub struct FoldersResponse {
    pub folders: Vec<FolderSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub struct FolderSummaryResponse {
    pub r#type: String,
    pub folder: String,
    pub name: Option<String>,
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
pub struct DocumentResponse {
    pub id: String,
    pub metadata: kataan_core::document::DocumentMetadata,
    pub markdown: String,
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

#[derive(Debug, Serialize)]
pub struct DiagnosticResponse {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/vault", get(vault))
        .route("/api/folders", get(folders))
        .route("/api/folder", get(folder_by_id))
        .route("/api/document", get(document_by_id))
        .route("/api/folders/:folder", get(folder))
        .route("/api/documents/*id", get(document))
        .route("/api/validate", post(validate))
        .route("/api/rebuild-indexes", post(rebuild_indexes))
        .with_state(state)
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultConfig>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    Ok(Json(loaded.index.clone()))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let mut folders = Vec::new();

    for (ty, folder) in &loaded.index.type_folders {
        let id = kataan_core::id::CanonicalId::parse(folder)
            .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
        let record = loaded.documents.get(&id);
        let document_count = loaded
            .documents
            .values()
            .filter(|document| {
                !document.is_folder_index && document.id.containing_folder() == folder
            })
            .count();
        folders.push(FolderSummaryResponse {
            r#type: ty.clone(),
            folder: folder.clone(),
            name: Some(
                record
                    .and_then(document_name)
                    .unwrap_or_else(|| title_from_id(folder)),
            ),
            document_count,
        });
    }

    Ok(Json(FoldersResponse { folders }))
}

pub async fn folder(
    State(state): State<AppState>,
    Path(folder): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = kataan_core::id::CanonicalId::parse(&folder)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let record = loaded
        .documents
        .get(&id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("folder `{folder}` does not exist")))?;
    let documents = direct_documents(&loaded, &id);
    let index = kataan_core::index::FolderIndex {
        name: document_name(record).unwrap_or_else(|| title_from_id(&folder)),
        description: None,
        default_type: Some(record.metadata.r#type.clone()),
        folder_checksum: None,
        documents: Vec::new(),
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

pub async fn document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DocumentResponse>, ApiError> {
    document_response(&state, &id).map(Json)
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
    state.reload().map_err(ApiError::from)?;
    Ok(Json(OkResponse { ok: true }))
}

fn document_response(state: &AppState, id: &str) -> Result<DocumentResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let record = {
        let loaded = read_loaded_vault(state)?;
        loaded
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError(anyhow::anyhow!("document `{id}` does not exist")))?
    };
    let markdown = std::fs::read_to_string(&record.markdown_path).map_err(|source| {
        ApiError(
            kataan_core::Error::Io {
                path: record.markdown_path.clone(),
                source,
            }
            .into(),
        )
    })?;

    Ok(DocumentResponse {
        id: id.as_str().to_owned(),
        metadata: record.metadata,
        markdown,
    })
}

fn canonical_folder_response(
    state: &AppState,
    id: &str,
) -> Result<CanonicalFolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let (record, folders, documents, markdown_path) = {
        let loaded = read_loaded_vault(state)?;
        let record = loaded
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError(anyhow::anyhow!("folder `{id}` does not exist")))?;
        if !record.is_folder_index {
            return Err(ApiError(anyhow::anyhow!("document `{id}` is not a folder")));
        }
        let folders = direct_folders(&loaded, &id);
        let documents = direct_documents(&loaded, &id);
        (record.clone(), folders, documents, record.markdown_path)
    };
    let markdown = std::fs::read_to_string(&markdown_path).ok();

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata: Some(record.metadata),
        markdown,
        folders,
        documents,
    })
}

fn read_loaded_vault(
    state: &AppState,
) -> Result<std::sync::RwLockReadGuard<'_, kataan_core::vault::LoadedVault>, ApiError> {
    state
        .vault
        .read()
        .map_err(|_| ApiError(anyhow::anyhow!("vault lock poisoned")))
}

fn direct_folders(
    loaded: &kataan_core::vault::LoadedVault,
    id: &kataan_core::id::CanonicalId,
) -> Vec<FolderChildResponse> {
    loaded
        .graph
        .children_of(id)
        .into_iter()
        .filter_map(|child_id| {
            let child = loaded.documents.get(&child_id)?;
            child.is_folder_index.then(|| FolderChildResponse {
                id: child_id.as_str().to_owned(),
                name: document_name(child).unwrap_or_else(|| title_from_id(child_id.as_str())),
                has_index: true,
            })
        })
        .collect()
}

fn direct_documents(
    loaded: &kataan_core::vault::LoadedVault,
    id: &kataan_core::id::CanonicalId,
) -> Vec<FolderDocumentResponse> {
    loaded
        .graph
        .children_of(id)
        .into_iter()
        .filter_map(|child_id| {
            let child = loaded.documents.get(&child_id)?;
            (!child.is_folder_index).then(|| FolderDocumentResponse {
                id: child_id.as_str().to_owned(),
                slug: child_id.slug().to_owned(),
                markdown: child
                    .markdown_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                toml: child
                    .toml_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

fn document_name(document: &kataan_core::vault::DocumentRecord) -> Option<String> {
    document
        .metadata
        .aliases
        .first()
        .cloned()
        .or_else(|| document.metadata.labels.first().cloned())
}

fn title_from_id(id: &str) -> String {
    id.rsplit('/')
        .next()
        .unwrap_or(id)
        .replace('-', " ")
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
        Router,
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn vault_endpoint_returns_root_index() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "GET", "/api/vault").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn folders_endpoint_returns_folder_list() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "GET", "/api/folders").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn folder_endpoint_returns_folder_index() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "GET", "/api/folders/type").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn canonical_folder_endpoint_returns_nested_folder() {
        let root = test_vault();
        fs::create_dir_all(root.join("projects/snappy/sows")).unwrap();
        fs::write(root.join("projects/snappy/index.md"), "# Snappy\n").unwrap();
        fs::write(
            root.join("projects/snappy/index.toml"),
            r#"type = "project"
name = "Snappy"
markdown = "index.md"
"#,
        )
        .unwrap();
        fs::write(root.join("projects/snappy/sows/index.md"), "# SOWs\n").unwrap();
        fs::write(
            root.join("projects/snappy/sows/index.toml"),
            r#"type = "project"
name = "SOWs"
markdown = "index.md"
"#,
        )
        .unwrap();
        fs::write(root.join("projects/snappy/sows/demo.md"), "# Demo\n").unwrap();
        fs::write(
            root.join("projects/snappy/sows/demo.toml"),
            r#"type = "project"
markdown = "demo.md"
"#,
        )
        .unwrap();
        let app = test_app(&root);

        let response = request(app, "GET", "/api/folder?id=projects%2Fsnappy%2Fsows").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn query_document_endpoint_returns_nested_document() {
        let root = test_vault();
        fs::create_dir_all(root.join("projects/snappy/sows/otp-travel")).unwrap();
        fs::write(
            root.join("projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429.md"),
            "# Demo\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429.toml"),
            r#"type = "project"
markdown = "HU-otp-travel-POC-SOW1-260429.md"
"#,
        )
        .unwrap();
        let app = test_app(&root);

        let response = request(
            app,
            "GET",
            "/api/document?id=projects%2Fsnappy%2Fsows%2Fotp-travel%2FHU-otp-travel-POC-SOW1-260429",
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn document_endpoint_returns_document() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "GET", "/api/documents/type/project").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn validate_endpoint_returns_diagnostics() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "POST", "/api/validate").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rebuild_indexes_endpoint_repairs_indexes() {
        let root = test_vault();
        let app = test_app(&root);

        let response = request(app, "POST", "/api/rebuild-indexes").await;

        assert_eq!(response.status(), StatusCode::OK);

        fs::remove_dir_all(root).unwrap();
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
}
