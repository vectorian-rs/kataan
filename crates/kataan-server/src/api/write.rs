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

/// One edge, named by its three parts. Deserialized straight into the shape
/// `mutate` takes, as MCP does with the same names.
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
        let _writer = state.lock_writes();

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
    Json(request): Json<kataan_core::mutate::NewDocument>,
) -> Result<(StatusCode, Json<CreatedResponse>), ApiError> {
    reject_cross_site(&headers)?;
    let id = write_action(state, move |root| {
        kataan_core::mutate::create_document(root, request)
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
    Json(edit): Json<kataan_core::mutate::DocumentEdit>,
) -> Result<Json<OkResponse>, ApiError> {
    reject_cross_site(&headers)?;
    let id = CanonicalId::parse(&id).map_err(ApiError::bad_request)?;
    write_action(state, move |root| {
        kataan_core::mutate::update_document(root, &id, edit.body, edit.patch)
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
