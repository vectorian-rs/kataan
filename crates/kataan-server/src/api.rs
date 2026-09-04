use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use kataan_core::{id::CanonicalId, title::title_from_id};

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

/// A resolved document: its canonical id plus enough context to fetch or route
/// to it without a second lookup. Shared by both resolve endpoints, which
/// differ only in how the id is looked up.
#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub id: String,
    pub folder: String,
    pub type_folder: String,
    pub is_folder_index: bool,
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticResponse>,
}

#[derive(Debug, Deserialize)]
pub struct IdQuery {
    pub id: String,
    /// Which syntax-highlighting theme fenced code blocks render with. Absent
    /// means dark, matching the file preview.
    pub theme: Option<String>,
}

/// Just the theme, for routes that take their id from the path.
#[derive(Debug, Deserialize)]
pub struct ThemeQuery {
    pub theme: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NeighborsQuery {
    pub id: String,
    pub predicate: Option<String>,
    #[serde(default)]
    pub direction: kataan_core::query::Direction,
}

/// `types` and `predicates` accept comma-separated lists; absent or empty means
/// no filter on that axis.
#[derive(Debug, Deserialize)]
pub struct SubgraphQuery {
    pub types: Option<String>,
    pub predicates: Option<String>,
}

fn comma_separated(value: Option<&String>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: String,
    pub theme: Option<String>,
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
        .route("/resolve-path", get(resolve_path))
        .route("/documents", get(documents))
        .route("/graph/neighbors", get(neighbors))
        .route("/graph/subgraph", get(subgraph))
        .route("/schema/:kind", get(schema))
        .route("/ontology", get(ontology))
        .route("/folders/:folder", get(folder))
        .route("/documents/*id", get(document))
        .route("/documents", post(create_document))
        .route("/documents/*id", patch(update_document))
        .route("/edges", post(add_edge))
        .route("/edges", delete(remove_edge))
        .route("/edges", put(replace_edges))
        .route("/validate", post(validate))
        .route("/rebuild-indexes", post(rebuild_indexes))
        // Own the miss: an unknown /api/* path is a 404, not the SPA shell that
        // the outer fallback would otherwise serve (axum propagates it here).
        .fallback(|| async { StatusCode::NOT_FOUND });

    let app = Router::new().nest("/api", api).with_state(state);

    // In the default API-only build, make `/` explain why the UI is not here
    // instead of returning an opaque 404. The embedded-UI build owns `/` via the
    // SPA fallback below.
    #[cfg(not(feature = "embed-ui"))]
    let app = app.route("/", get(api_only_root));
    // With `--features embed-ui`, serve the embedded SPA for any non-API path.
    #[cfg(feature = "embed-ui")]
    let app = app.fallback(crate::ui::serve);

    app
}

#[cfg(not(feature = "embed-ui"))]
async fn api_only_root() -> &'static str {
    "Kataan API server is running.\n\nThis binary was built without the embedded web UI, so `/` is not the app.\nUse `/api/health` for the API, run `bun run dev:web` for the web UI (default http://127.0.0.1:3003), or build/install `kataan-server` with the `embed-ui` feature.\n"
}

/// Run blocking work on the blocking pool instead of an async worker thread.
///
/// Every handler below does synchronous filesystem I/O, and several also do
/// real CPU work — syntax highlighting a 10 MB file, walking and rewriting the
/// whole vault. On the async runtime that occupies a worker for the duration,
/// and tokio has one worker per core. A handful of concurrent highlight
/// requests could therefore stall *every* route, `/api/health` included, which
/// is the opposite of what a health check is for.
async fn blocking<T, F>(work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        // The only ways this resolves to an error are a panic in the closure or
        // runtime shutdown. Neither is the caller's fault, so neither is a 4xx.
        Err(error) => Err(ApiError::from(anyhow::anyhow!(
            "request handler failed: {error}"
        ))),
    }
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
    // SQLite is a blocking API however small the query.
    blocking(move || state.search.search(&query).map_err(ApiError::from))
        .await
        .map(Json)
}

pub async fn search_status(
    State(state): State<AppState>,
) -> Result<Json<kataan_search::SearchStatus>, ApiError> {
    blocking(move || {
        kataan_search::SearchIndex::status_for_vault(state.vault_path.as_ref())
            .map_err(ApiError::from)
    })
    .await
    .map(Json)
}

pub async fn search_reindex(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<kataan_search::ReindexResponse>, ApiError> {
    reject_cross_site(&headers)?;
    // Reads and rewrites the index for every document in the vault.
    blocking(move || {
        let loaded = read_loaded_vault(&state)?;
        state.search.reindex_loaded(&loaded).map_err(ApiError::from)
    })
    .await
    .map(Json)
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultConfig>, ApiError> {
    Ok(Json(read_loaded_vault(&state)?.index.clone()))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let document_counts = document_counts_by_type_folder(&loaded);
    let mut folders = Vec::new();

    for (ty, folder) in &loaded.index.type_folders {
        let id = kataan_core::id::CanonicalId::parse(folder)
            .map_err(|source| ApiError::from(anyhow::anyhow!(source)))?;
        let record = loaded.documents.get(&id);
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
            document_count: document_counts.get(folder).copied().unwrap_or(0),
        });
    }

    Ok(Json(FoldersResponse { folders }))
}

pub async fn folder(
    State(state): State<AppState>,
    Path(folder): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = kataan_core::id::CanonicalId::parse(&folder).map_err(ApiError::bad_request)?;
    let Some(record) = loaded.documents.get(&id) else {
        if is_file_backed_folder(&loaded, &id) {
            return Ok(Json(FolderResponse {
                folder,
                index: file_backed_folder_index(&loaded, &id),
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
        type_folders: Default::default(),
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
    blocking(move || canonical_folder_response(&state, &query.id))
        .await
        .map(Json)
}

pub async fn document_by_id(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> Result<Json<DocumentResponse>, ApiError> {
    blocking(move || document_response(&state, &query.id, query.theme.as_deref()))
        .await
        .map(Json)
}

pub async fn file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileResponse>, ApiError> {
    blocking(move || file_response(&state, &query.path))
        .await
        .map(Json)
}

pub async fn highlight_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<HighlightResponse>, ApiError> {
    // Reads up to 10 MB and tokenises it — the worst offender of the set.
    blocking(move || highlight_response(&state, &query.path, query.theme.as_deref()))
        .await
        .map(Json)
}

pub async fn raw_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<axum::response::Response, ApiError> {
    // Reads up to 50 MB into memory.
    blocking(move || raw_file_response(&state, &query.path)).await
}

pub async fn resolve_path(
    State(state): State<AppState>,
    Query(query): Query<PathQuery>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = loaded
        .resolve_path(&query.path)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "path `{}` does not resolve to a document in this vault",
                query.path
            ))
        })?
        .clone();
    Ok(Json(resolved(&loaded, &id)))
}

/// The shared projection behind both resolve endpoints: they differ only in how
/// the id was found, not in what a caller gets back.
fn resolved(loaded: &kataan_core::vault::LoadedVault, id: &CanonicalId) -> ResolveResponse {
    ResolveResponse {
        id: id.as_str().to_owned(),
        folder: id.containing_folder().to_owned(),
        type_folder: id.top_level_folder().to_owned(),
        is_folder_index: loaded
            .documents
            .get(id)
            .is_some_and(|record| record.is_folder_index),
    }
}

/// Filters arrive as a query string; `ids` and `labels` accept comma-separated
/// lists.
#[derive(Debug, Deserialize)]
pub struct DocumentsQuery {
    pub ids: Option<String>,
    pub r#type: Option<String>,
    pub status: Option<String>,
    pub labels: Option<String>,
    pub path_prefix: Option<String>,
    pub linked_to: Option<String>,
    pub predicate: Option<String>,
    #[serde(default)]
    pub direction: kataan_core::query::Direction,
    #[serde(default)]
    pub include: kataan_core::query::Include,
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}

pub async fn documents(
    State(state): State<AppState>,
    Query(query): Query<DocumentsQuery>,
) -> Result<Json<kataan_core::query::DocumentPage>, ApiError> {
    // `include=markdown` reads one file per matched document.
    blocking(move || documents_response(&state, query))
        .await
        .map(Json)
}

fn documents_response(
    state: &AppState,
    query: DocumentsQuery,
) -> Result<kataan_core::query::DocumentPage, ApiError> {
    let loaded = read_loaded_vault(state)?;
    let request = kataan_core::query::DocumentQuery {
        ids: comma_separated(query.ids.as_ref()),
        r#type: query.r#type,
        status: query.status,
        labels: comma_separated(query.labels.as_ref()),
        path_prefix: query.path_prefix,
        linked_to: query.linked_to.map(|id| kataan_core::query::LinkedTo {
            id,
            predicate: query.predicate,
            direction: query.direction,
        }),
        include: query.include,
        limit: query.limit,
        offset: query.offset,
    };
    kataan_core::query::documents(&loaded, &request).map_err(|error| match error {
        // A bad filter or an over-limit result is the caller's mistake.
        kataan_core::Error::InvalidRequest(message) => ApiError::bad_request(message),
        other => ApiError::from(other),
    })
}

pub async fn neighbors(
    State(state): State<AppState>,
    Query(query): Query<NeighborsQuery>,
) -> Result<Json<kataan_core::query::Neighbors>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = kataan_core::id::CanonicalId::parse(&query.id).map_err(ApiError::bad_request)?;
    if !loaded.documents.contains_key(&id) {
        return Err(ApiError::not_found(format!(
            "document `{id}` does not exist"
        )));
    }
    kataan_core::query::neighbors(&loaded, &id, query.predicate.as_deref(), query.direction)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn subgraph(
    State(state): State<AppState>,
    Query(query): Query<SubgraphQuery>,
) -> Result<Json<kataan_core::query::Subgraph>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let types = comma_separated(query.types.as_ref());
    let predicates = comma_separated(query.predicates.as_ref());
    Ok(Json(kataan_core::query::subgraph(
        &loaded,
        &types,
        &predicates,
    )))
}

pub async fn document(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ThemeQuery>,
) -> Result<Json<DocumentResponse>, ApiError> {
    blocking(move || document_response(&state, &id, query.theme.as_deref()))
        .await
        .map(Json)
}

/// The vault's whole model — types, their declared fields, and the legal edges
/// between them — in one call.
pub async fn ontology(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::schema::OntologyResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    Ok(Json(kataan_core::schema::ontology_response(&loaded)))
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

/// Refuse a state-changing request that a foreign page triggered.
///
/// Neither write route takes a body, so a plain `<form method="POST">` on any
/// site the user visits is a CORS "simple request": no preflight, no `Origin`
/// enforcement by the browser, and the side effect fires. Binding to localhost
/// does not help. Non-browser callers (curl, the CLI) send neither header and
/// are unaffected.
fn reject_cross_site(headers: &HeaderMap) -> Result<(), ApiError> {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "same-origin" && site != "none" {
            return Err(ApiError::forbidden(format!(
                "cross-site `{site}` request refused"
            )));
        }
        return Ok(());
    }
    // Older browsers send `Origin` but not `Sec-Fetch-Site`. A request from a
    // real page always carries one of the two; a bare tool carries neither.
    if headers.contains_key(header::ORIGIN) {
        return Err(ApiError::forbidden(
            "cross-origin request refused".to_owned(),
        ));
    }
    Ok(())
}

/// A new document. Mirrors the MCP `create_document` arguments exactly, so the
/// two surfaces cannot drift into accepting different things.
#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub r#type: String,
    pub title: String,
    pub body: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub status: Option<String>,
    pub actor: Option<String>,
    pub occurred_at: Option<String>,
    /// Extra top-level sidecar keys, validated against the type's `[nodes.*]`
    /// schema before anything is written. `GET /api/schema/<type>` describes
    /// what belongs here.
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, toml::Value>,
}

/// A partial update. An omitted field means "leave it alone", which is why
/// every one of these is an `Option` — including `body`.
#[derive(Debug, Deserialize)]
pub struct UpdateDocumentRequest {
    pub body: Option<String>,
    pub status: Option<String>,
    pub occurred_at: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub actor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EdgeRequest {
    pub source: String,
    pub predicate: String,
    pub target: String,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceEdgesRequest {
    pub source: String,
    pub predicate: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatedResponse {
    pub id: String,
}

/// Perform a vault mutation: one writer at a time, off the async runtime, with
/// the in-memory vault and the search index refreshed afterwards.
///
/// Every write route goes through here so none of them can forget a step. A
/// stale search index or a stale `LoadedVault` after a write is not a crash —
/// it is the API quietly serving what used to be true.
async fn write_action<T, F>(state: AppState, work: F) -> Result<T, ApiError>
where
    F: FnOnce(&std::path::Path) -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    blocking(move || {
        let _writer = state
            .writes
            .lock()
            .map_err(|_| ApiError::from(anyhow::anyhow!("write lock poisoned")))?;

        let result = work(state.vault_path.as_ref())?;

        // The vault on disk changed. Refresh what the read paths serve before
        // returning, so a caller that writes and immediately reads sees its own
        // write rather than the previous state.
        if let Err(error) = state.reload() {
            error!(error = %error, "vault reload after write failed; reads are stale");
            return Err(ApiError::from(error));
        }
        match read_loaded_vault(&state) {
            Ok(loaded) => {
                if let Err(error) = state.search.reindex_loaded(&loaded) {
                    // The write succeeded; only the index is behind. Saying so
                    // is better than failing a write that actually landed.
                    warn!(error = %error, "search reindex after write failed; index is stale");
                }
            }
            Err(error) => warn!(error = ?error, "search reindex after write skipped"),
        }
        Ok(result)
    })
    .await
}

pub async fn create_document(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), ApiError> {
    reject_cross_site(&headers)?;
    let id = write_action(state, move |root| {
        kataan_core::mutate::create_document(
            root,
            kataan_core::mutate::NewDocument {
                r#type: request.r#type,
                title: request.title,
                body: request.body,
                parent: request.parent,
                aliases: request.aliases,
                labels: request.labels,
                status: request.status,
                actor: request.actor,
                occurred_at: request.occurred_at,
                extra: request.fields,
            },
        )
        .map_err(write_error)
    })
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse {
            id: id.as_str().to_owned(),
        }),
    ))
}

pub async fn update_document(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    let id = CanonicalId::parse(&id).map_err(ApiError::bad_request)?;
    write_action(state, move |root| {
        kataan_core::mutate::update_document(
            root,
            &id,
            request.body,
            kataan_core::mutate::DocumentPatch {
                status: request.status,
                occurred_at: request.occurred_at,
                aliases: request.aliases,
                labels: request.labels,
                actor: request.actor,
            },
        )
        .map_err(write_error)
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

pub async fn add_edge(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<EdgeRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    let (source, target) = edge_endpoints(&request.source, &request.target)?;
    write_action(state, move |root| {
        kataan_core::mutate::add_edge(root, &source, &request.predicate, &target)
            .map_err(write_error)
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

pub async fn remove_edge(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(request): Query<EdgeRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    let (source, target) = edge_endpoints(&request.source, &request.target)?;
    write_action(state, move |root| {
        kataan_core::mutate::remove_edge(root, &source, &request.predicate, &target)
            .map_err(write_error)
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

pub async fn replace_edges(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ReplaceEdgesRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    let source = CanonicalId::parse(&request.source).map_err(ApiError::bad_request)?;
    let targets = request
        .targets
        .iter()
        .map(|target| CanonicalId::parse(target).map_err(ApiError::bad_request))
        .collect::<Result<Vec<_>, _>>()?;
    write_action(state, move |root| {
        kataan_core::mutate::replace_edges_for_predicate(
            root,
            &source,
            &request.predicate,
            &targets,
        )
        .map_err(write_error)
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

fn edge_endpoints(source: &str, target: &str) -> Result<(CanonicalId, CanonicalId), ApiError> {
    Ok((
        CanonicalId::parse(source).map_err(ApiError::bad_request)?,
        CanonicalId::parse(target).map_err(ApiError::bad_request)?,
    ))
}

/// A rejected write is the caller's mistake, not a server fault: an unknown
/// type, a malformed timestamp, a field that violates the type's schema. Those
/// arrive as `InvalidRequest` and must not surface as 500.
fn write_error(error: kataan_core::Error) -> ApiError {
    match error {
        kataan_core::Error::InvalidRequest(message) => ApiError::bad_request(message),
        other => ApiError::from(other),
    }
}

pub async fn validate(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<ValidateResponse>, ApiError> {
    reject_cross_site(&headers)?;
    debug!(vault = %state.vault_path.display(), "validating vault");
    // Walks every folder and parses every sidecar in the vault.
    blocking(move || {
        let report =
            kataan_core::validate::validate(state.vault_path.as_ref()).map_err(ApiError::from)?;
        let ok = report.is_ok();
        let diagnostics = report
            .diagnostics
            .iter()
            .map(DiagnosticResponse::from)
            .collect();

        match state.reload() {
            Ok(()) => {
                debug!(vault = %state.vault_path.display(), "reloaded vault after validation")
            }
            Err(error) => {
                debug!(vault = %state.vault_path.display(), error = %error, "vault reload after validation skipped")
            }
        }

        Ok(ValidateResponse { ok, diagnostics })
    })
    .await
    .map(Json)
}

pub async fn rebuild_indexes(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    info!(vault = %state.vault_path.display(), "rebuilding indexes");
    // Rewrites every folder index in the vault.
    blocking(move || {
        kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref()).map_err(ApiError::from)?;
        state.reload().map_err(ApiError::from)?;
        info!(vault = %state.vault_path.display(), "reloaded vault after rebuild");
        Ok(OkResponse { ok: true })
    })
    .await
    .map(Json)
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

    pub fn forbidden(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::FORBIDDEN, message)
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
