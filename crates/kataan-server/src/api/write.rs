//! The write path: create, update, and the three edge mutations.
//!
//! Every route here goes through [`write_action`], which is the point: a write
//! that forgot to refresh the in-memory vault or the search index is not a
//! crash, it is the API quietly serving what used to be true.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use kataan_core::id::CanonicalId;

use crate::state::AppState;

use super::{blocking, core_error, read_loaded_vault, reject_cross_site, ApiError, OkResponse};

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
    /// Custom sidecar keys to set. A JSON `null` removes the key; keys not
    /// mentioned are left alone.
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, serde_json::Value>,
    /// The `updated_at` the caller last read. When present the write is
    /// refused with `409` unless the document still carries it, so an editor
    /// cannot silently overwrite a change it never saw.
    pub expected_updated_at: Option<String>,
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
/// `work` returns its result together with the document it changed, because a
/// create does not know its own id until it has run.
async fn write_action<T, F>(state: AppState, work: F) -> Result<T, ApiError>
where
    F: FnOnce(&std::path::Path) -> Result<(T, CanonicalId), ApiError> + Send + 'static,
    T: Send + 'static,
{
    blocking(move || {
        let _writer = state
            .writes
            .lock()
            .map_err(|_| ApiError::from(anyhow::anyhow!("write lock poisoned")))?;

        let (result, changed) = work(state.vault_path.as_ref())?;

        // The lock is deliberately held through the refresh below, not just the
        // mutation. `reload()` is read-disk-then-swap, so releasing early let
        // two writers interleave: a slow reload could store a snapshot taken
        // before a second write and silently discard it, and the second
        // writer's `refresh_document` could then look up its own new document
        // in that stale snapshot, not find it, and delete it from the search
        // index — on a request that returned 201.
        //
        // Serialising the refresh costs the other writer ~190ms of queueing.
        // Correct reads are worth more than that.

        // The vault on disk changed. Refresh what the read paths serve before
        // returning, so a caller that writes and immediately reads sees its own
        // write rather than the previous state.
        if let Err(error) = state.reload() {
            error!(error = %error, "vault reload after write failed; reads are stale");
            return Err(ApiError::from(error));
        }
        match read_loaded_vault(&state) {
            Ok(loaded) => {
                // Only the document the write touched. Rebuilding the whole
                // index to record one change was the largest remaining cost of
                // a write; a full rebuild is the fallback for an index that
                // does not exist yet or predates the current schema.
                let refreshed = state
                    .search
                    .refresh_document(&loaded, &changed)
                    .unwrap_or(false);
                if !refreshed {
                    if let Err(error) = state.search.reindex_loaded(&loaded) {
                        // The write succeeded; only the index is behind. Saying
                        // so beats failing a write that actually landed.
                        warn!(error = %error, "search reindex after write failed; index is stale");
                    }
                }
            }
            Err(error) => warn!(error = ?error, "search refresh after write skipped"),
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
        .map_err(core_error)
        .map(|id| (id.clone(), id))
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
                expected_updated_at: request.expected_updated_at,
                fields: request
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), kataan_core::convert::json_to_toml(value)))
                    .collect(),
                status: request.status,
                occurred_at: request.occurred_at,
                aliases: request.aliases,
                labels: request.labels,
                actor: request.actor,
            },
        )
        .map_err(core_error)
        .map(|()| ((), id))
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
            .map_err(core_error)
            .map(|()| ((), source))
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
            .map_err(core_error)
            .map(|()| ((), source))
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
        .map_err(core_error)
        .map(|()| ((), source))
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
