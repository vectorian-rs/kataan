use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::state::AppState;

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
pub struct ResolveQuery {
    pub r#type: String,
    pub token: String,
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
        .route("/api/resolve", get(resolve_route))
        .route("/api/schema/:kind", get(schema))
        .route("/api/folders/:folder", get(folder))
        .route("/api/documents/*id", get(document))
        .route("/api/validate", post(validate))
        .route("/api/rebuild-indexes", post(rebuild_indexes))
        .with_state(state)
}

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let loaded = state.vault.read().is_ok();
    Json(HealthResponse { ok: true, loaded })
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultConfig>, ApiError> {
    if let Ok(loaded) = read_loaded_vault(&state) {
        return Ok(Json(loaded.index.clone()));
    }

    let vault =
        kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)?;
    Ok(Json(vault.index))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let mut folders = Vec::new();

    if let Ok(loaded) = read_loaded_vault(&state) {
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
        push_code_folder_if_needed(
            &state,
            loaded
                .index
                .type_folders
                .values()
                .any(|folder| kataan_core::constants::is_code_folder(folder)),
            &mut folders,
        );
        return Ok(Json(FoldersResponse { folders }));
    }

    let vault =
        kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)?;
    for (ty, folder) in &vault.index.type_folders {
        let index = vault.load_folder_index(folder).ok();
        folders.push(FolderSummaryResponse {
            r#type: ty.clone(),
            folder: folder.clone(),
            name: index
                .as_ref()
                .map(|index| index.name.clone())
                .or_else(|| Some(title_from_id(folder))),
            document_count: index.map(|index| index.documents.len()).unwrap_or_default(),
        });
    }
    push_code_folder_if_needed(
        &state,
        vault
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
    let Ok(loaded) = read_loaded_vault(&state) else {
        return filesystem_folder_response(&state, &folder).map(Json);
    };
    let id = kataan_core::id::CanonicalId::parse(&folder)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let Some(record) = loaded.documents.get(&id) else {
        if kataan_core::constants::is_code_path(id.as_str()) {
            return Ok(Json(FolderResponse {
                folder,
                index: empty_code_folder_index(id.as_str()),
                documents: Vec::new(),
            }));
        }
        return Err(ApiError(anyhow::anyhow!(
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

pub async fn resolve_route(
    State(state): State<AppState>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = loaded
        .resolve_route_token(&query.r#type, &query.token)
        .cloned()
        .ok_or_else(|| {
            ApiError(anyhow::anyhow!(
                "route token `{}` for type `{}` does not resolve",
                query.token,
                query.r#type
            ))
        })?;
    let document = loaded
        .documents
        .get(&id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("document `{id}` does not exist")))?;
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
    let response = kataan_core::schema::schema_response(&kind, loaded.as_ref())
        .ok_or_else(|| ApiError(anyhow::anyhow!("unknown schema kind `{kind}`")))?;
    Ok(Json(response))
}

pub async fn validate(State(state): State<AppState>) -> Result<Json<ValidateResponse>, ApiError> {
    debug!(vault = %state.vault_path.display(), "validating vault");
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

fn document_response(state: &AppState, id: &str) -> Result<DocumentResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;

    if let Ok(loaded) = read_loaded_vault(state) {
        let record = loaded
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError(anyhow::anyhow!("document `{id}` does not exist")))?;
        return document_response_from_parts(&id, record.metadata, &record.markdown_path);
    }

    filesystem_document_response(state, &id)
}

fn canonical_folder_response(
    state: &AppState,
    id: &str,
) -> Result<CanonicalFolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let Ok(loaded) = read_loaded_vault(state) else {
        return filesystem_canonical_folder_response(state, &id);
    };
    let (record, folders, documents, markdown_path) = {
        let Some(record) = loaded.documents.get(&id).cloned() else {
            if kataan_core::constants::is_code_path(id.as_str()) {
                return Ok(CanonicalFolderResponse {
                    id: id.as_str().to_owned(),
                    metadata: None,
                    markdown: None,
                    folders: direct_code_folders(state, id.as_str())?,
                    documents: Vec::new(),
                    files: folder_files(state, &id, &[])?,
                });
            }
            return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
        };
        if !record.is_folder_index {
            return Err(ApiError(anyhow::anyhow!("document `{id}` is not a folder")));
        }
        let folders = direct_folders(&loaded, &id);
        let documents = direct_documents(&loaded, &id);
        (record.clone(), folders, documents, record.markdown_path)
    };
    let markdown = std::fs::read_to_string(&markdown_path).ok();
    let files = folder_files(state, &id, &documents)?;

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata: Some(record.metadata),
        markdown,
        folders,
        documents,
        files,
    })
}

fn filesystem_document_response(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
) -> Result<DocumentResponse, ApiError> {
    let toml_path = state.vault_path.join(id.toml_path());
    let metadata = read_document_metadata_if_valid(&toml_path).ok_or_else(|| {
        ApiError(anyhow::anyhow!(
            "document `{id}` cannot be loaded because its TOML metadata is invalid"
        ))
    })?;
    let markdown_path = state.vault_path.join(id.folder()).join(&metadata.markdown);
    document_response_from_parts(id, metadata, &markdown_path)
}

fn document_response_from_parts(
    id: &kataan_core::id::CanonicalId,
    metadata: kataan_core::document::DocumentMetadata,
    markdown_path: &std::path::Path,
) -> Result<DocumentResponse, ApiError> {
    let markdown = std::fs::read_to_string(markdown_path).map_err(|source| {
        ApiError(
            kataan_core::Error::Io {
                path: markdown_path.to_path_buf(),
                source,
            }
            .into(),
        )
    })?;

    Ok(DocumentResponse {
        id: id.as_str().to_owned(),
        type_folder: id.top_level_folder().to_owned(),
        route_token: kataan_core::vault::route_token_for_id(id),
        metadata,
        markdown,
    })
}

fn filesystem_folder_response(state: &AppState, folder: &str) -> Result<FolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(folder)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let response = filesystem_canonical_folder_response(state, &id)?;
    Ok(FolderResponse {
        folder: response.id.clone(),
        index: kataan_core::index::FolderIndex {
            name: response
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.aliases.first().cloned())
                .unwrap_or_else(|| title_from_id(&response.id)),
            description: None,
            default_type: response.metadata.map(|metadata| metadata.r#type),
            folder_checksum: None,
            documents: Vec::new(),
        },
        documents: response.documents,
    })
}

fn filesystem_canonical_folder_response(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
) -> Result<CanonicalFolderResponse, ApiError> {
    if kataan_core::constants::is_code_path(id.as_str()) {
        return Ok(CanonicalFolderResponse {
            id: id.as_str().to_owned(),
            metadata: None,
            markdown: None,
            folders: direct_code_folders(state, id.as_str())?,
            documents: Vec::new(),
            files: folder_files(state, id, &[])?,
        });
    }

    let folder_path = state.vault_path.join(id.as_str());
    if !folder_path.is_dir() {
        return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
    }

    let metadata = read_document_metadata_if_valid(&folder_path.join("index.toml"));
    let markdown = std::fs::read_to_string(folder_path.join("index.md")).ok();
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
            folders.push(FolderChildResponse {
                id: format!("{}/{}", id.as_str(), name),
                name,
                has_index: path.join("index.md").exists() && path.join("index.toml").exists(),
            });
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || name == "index.md"
        {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{slug}.toml"));
        if read_document_metadata_if_valid(&toml_path).is_none() {
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
    let files = folder_files(state, id, &documents)?;

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata,
        markdown,
        folders,
        documents,
        files,
    })
}

fn folder_files(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
    documents: &[FolderDocumentResponse],
) -> Result<Vec<FolderFileResponse>, ApiError> {
    let folder_path = state.vault_path.join(id.as_str());
    let document_sidecars = documents
        .iter()
        .flat_map(|document| [document.markdown.as_str(), document.toml.as_str()])
        .collect::<std::collections::BTreeSet<_>>();
    let mut files = Vec::new();

    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "index.md" || name == "index.toml" || document_sidecars.contains(name.as_str()) {
            continue;
        }
        files.push(FolderFileResponse {
            extension: path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_owned),
            path: format!("{}/{}", id.as_str(), name),
            name,
        });
    }

    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

fn read_document_metadata_if_valid(
    path: &std::path::Path,
) -> Option<kataan_core::document::DocumentMetadata> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn push_code_folder_if_needed(
    state: &AppState,
    has_code_mapping: bool,
    folders: &mut Vec<FolderSummaryResponse>,
) {
    if !has_code_mapping
        && state
            .vault_path
            .join(kataan_core::constants::CODE_FOLDER)
            .is_dir()
    {
        folders.push(FolderSummaryResponse {
            r#type: kataan_core::constants::TYPE_CODE.to_owned(),
            folder: kataan_core::constants::CODE_FOLDER.to_owned(),
            name: Some("Code".to_owned()),
            document_count: 0,
        });
    }
}

fn empty_code_folder_index(id: &str) -> kataan_core::index::FolderIndex {
    kataan_core::index::FolderIndex {
        name: title_from_id(id),
        description: Some("Agent tools and code assets.".to_owned()),
        default_type: Some("code".to_owned()),
        folder_checksum: None,
        documents: Vec::new(),
    }
}

fn direct_code_folders(state: &AppState, id: &str) -> Result<Vec<FolderChildResponse>, ApiError> {
    let folder_path = state.vault_path.join(id);
    if !folder_path.is_dir() {
        return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
    }

    let mut folders = Vec::new();
    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        folders.push(FolderChildResponse {
            id: format!("{id}/{name}"),
            name,
            has_index: false,
        });
    }
    folders.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(folders)
}

fn read_loaded_vault(state: &AppState) -> Result<kataan_core::vault::LoadedVault, ApiError> {
    state
        .vault
        .read()
        .map_err(|_| ApiError(anyhow::anyhow!("vault lock poisoned")))
        .map(|vault| vault.clone())
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
        error!(error = %self.0, "api request failed");
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
