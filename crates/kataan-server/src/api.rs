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
    Ok(Json(open_vault(&state)?.index))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let vault = open_vault(&state)?;
    let mut folders = Vec::new();

    for (ty, folder) in &vault.index.type_folders {
        let index = vault.load_folder_index(folder).ok();
        let document_count = index
            .as_ref()
            .map(|index| index.documents.len())
            .unwrap_or_default();
        folders.push(FolderSummaryResponse {
            r#type: ty.clone(),
            folder: folder.clone(),
            name: index.map(|index| index.name),
            document_count,
        });
    }

    Ok(Json(FoldersResponse { folders }))
}

pub async fn folder(
    State(state): State<AppState>,
    Path(folder): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let vault = open_vault(&state)?;
    let index = vault.load_folder_index(&folder).map_err(ApiError::from)?;
    let documents = index
        .documents
        .iter()
        .map(|document| FolderDocumentResponse {
            id: format!("{folder}/{}", document.slug),
            slug: document.slug.clone(),
            markdown: document.markdown.clone(),
            toml: document.toml.clone(),
        })
        .collect();

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
    Ok(Json(OkResponse { ok: true }))
}

fn document_response(state: &AppState, id: &str) -> Result<DocumentResponse, ApiError> {
    let vault = open_vault(state)?;
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let document = vault.load_document(&id).map_err(ApiError::from)?;

    Ok(DocumentResponse {
        id: id.as_str().to_owned(),
        metadata: document.metadata,
        markdown: document.markdown,
    })
}

fn canonical_folder_response(
    state: &AppState,
    id: &str,
) -> Result<CanonicalFolderResponse, ApiError> {
    let vault = open_vault(state)?;
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let folder_path = vault.root.join(id.as_str());
    if !folder_path.is_dir() {
        return Err(ApiError(anyhow::anyhow!("folder `{}` does not exist", id)));
    }

    let folder_document = vault.load_document(&id).ok();
    let mut folders = Vec::new();
    let mut documents = Vec::new();

    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            let child_id = format!("{}/{}", id.as_str(), name);
            folders.push(FolderChildResponse {
                id: child_id,
                name,
                has_index: path.join("index.toml").exists() && path.join("index.md").exists(),
            });
            continue;
        }

        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || name == "index.md"
        {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{slug}.toml"));
        if !toml_path.exists() {
            continue;
        }
        documents.push(FolderDocumentResponse {
            id: format!("{}/{}", id.as_str(), slug),
            slug: slug.to_owned(),
            markdown: name,
            toml: format!("{slug}.toml"),
        });
    }

    folders.sort_by(|left, right| left.id.cmp(&right.id));
    documents.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata: folder_document
            .as_ref()
            .map(|document| document.metadata.clone()),
        markdown: folder_document.map(|document| document.markdown),
        folders,
        documents,
    })
}

fn open_vault(state: &AppState) -> Result<kataan_core::vault::Vault, ApiError> {
    kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)
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
        fs::create_dir_all(root.join("projects/snappy/sows")).unwrap();
        fs::write(root.join("projects/snappy/sows/demo.md"), "# Demo\n").unwrap();
        fs::write(
            root.join("projects/snappy/sows/demo.toml"),
            r#"type = "project"
markdown = "demo.md"
"#,
        )
        .unwrap();
        let app = test_app(&root);

        let response = request(
            app,
            "GET",
            "/api/document?id=projects%2Fsnappy%2Fsows%2Fdemo",
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
        router(AppState::new(root.to_path_buf()))
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
