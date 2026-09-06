use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use kataan_core::{
    title::title_from_id,
    vault::{DocumentRecord, LoadedVault},
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub kind: Option<String>,
    #[serde(rename = "type")]
    pub type_filter: Option<String>,
    pub status: Option<String>,
    pub facet: Option<String>,
    pub path_prefix: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub mode: String,
    pub results: Vec<SearchResult>,
    pub facets: Vec<SearchFacetCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub kind: String,
    pub id: Option<String>,
    pub path: String,
    pub title: Option<String>,
    pub r#type: Option<String>,
    pub status: Option<String>,
    pub extension: Option<String>,
    pub facets: Vec<String>,
    pub snippet: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchFacetCount {
    pub facet: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchStatus {
    pub index_path: String,
    pub exists: bool,
    pub item_count: usize,
    pub document_count: usize,
    pub folder_count: usize,
    pub last_indexed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexResponse {
    pub ok: bool,
    pub index_path: String,
    pub item_count: usize,
    pub document_count: usize,
    pub folder_count: usize,
    pub indexed_at: String,
}

#[derive(Debug, Clone)]
pub struct SearchIndex {
    path: PathBuf,
}

impl SearchIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create search index directory `{}`",
                    parent.display()
                )
            })?;
        }
        let connection = match Self::open_configured(&path) {
            Ok(connection) => connection,
            // The index is derived from the vault and lives at an opaque
            // cache path a user cannot reasonably find, so a corrupt file
            // would otherwise be a permanent, unfixable failure. Nothing is
            // lost by recreating it.
            Err(_) => {
                std::fs::remove_file(&path).ok();
                Self::open_configured(&path)
                    .with_context(|| format!("failed to open search index `{}`", path.display()))?
            }
        };
        create_schema(&connection)?;
        Ok(Self { path })
    }

    /// Open and configure a connection, surfacing a corrupt file as an error.
    fn open_configured(path: &Path) -> Result<Connection> {
        let connection = Connection::open(path)?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    pub fn open_default(vault_root: impl AsRef<Path>) -> Result<Self> {
        Self::open(default_index_path(vault_root.as_ref()))
    }

    /// Resolve the default index path without touching disk, so callers can
    /// cache a handle at startup and let the SQLite file be created lazily on
    /// first use (via [`connect`](Self::connect)). Unlike [`open_default`], this
    /// neither creates the directory nor the database, so it never fails and
    /// leaves the "index exists" status accurate until the index is first used.
    pub fn at_default_path(vault_root: impl AsRef<Path>) -> Self {
        Self {
            path: default_index_path(vault_root.as_ref()),
        }
    }

    pub fn status_for_vault(vault_root: impl AsRef<Path>) -> Result<SearchStatus> {
        Self::status_at_path(default_index_path(vault_root.as_ref()))
    }

    pub fn status_at_path(path: impl AsRef<Path>) -> Result<SearchStatus> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(SearchStatus {
                index_path: path.display().to_string(),
                exists: false,
                item_count: 0,
                document_count: 0,
                folder_count: 0,
                last_indexed_at: None,
            });
        }

        let index = Self::open(&path)?;
        index.status()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn reindex_loaded(&self, loaded: &LoadedVault) -> Result<ReindexResponse> {
        let mut connection = self.open_connection()?;
        let indexed_at = kataan_core::time::unix_timestamp_string();
        let transaction = connection.transaction()?;

        // Drop and recreate rather than DELETE, so an index built on an older
        // schema is rebuilt with the current columns instead of failing inserts.
        transaction.execute_batch(
            "DROP TABLE IF EXISTS search_fts;
             DROP TABLE IF EXISTS search_facets;
             DROP TABLE IF EXISTS search_items;
             DROP TABLE IF EXISTS search_metadata;",
        )?;
        create_schema(&transaction)?;

        let mut item_count = 0usize;
        let mut document_count = 0usize;
        let mut folder_count = 0usize;

        for record in loaded.documents.values() {
            let markdown = loaded
                .read_markdown(&record.id)
                .with_context(|| format!("failed to read markdown for `{}`", record.id))?;
            let item = SearchItem::from_document_record(loaded, record, &markdown)?;

            insert_item(&transaction, &item)?;
            item_count += 1;
            match item.kind {
                Kind::Folder => folder_count += 1,
                Kind::Document => document_count += 1,
            }
        }

        transaction.execute(
            "INSERT OR REPLACE INTO search_metadata(key, value) VALUES (?1, ?2)",
            params!["last_indexed_at", indexed_at],
        )?;
        transaction.commit()?;

        Ok(ReindexResponse {
            ok: true,
            index_path: self.path.display().to_string(),
            item_count,
            document_count,
            folder_count,
            indexed_at,
        })
    }

    /// Bring one document's entry up to date without rebuilding the index.
    ///
    /// A write changes one document, and `reindex_loaded` responded by dropping
    /// all four tables and re-reading every markdown body in the vault — 766
    /// file reads and 766 inserts to record one change. Every mutation on every
    /// surface ends in a refresh, so that was the largest remaining cost of a
    /// write.
    ///
    /// Everything is keyed by `item_key`, so the scoped delete is exact. An id
    /// no longer present in the vault is simply removed.
    ///
    /// Returns `false` when the index could not be updated in place — it does
    /// not exist yet, or predates the current schema, in which case the columns
    /// this inserts into may be absent. The caller falls back to a full
    /// reindex, which is what `reindex_loaded`'s drop-and-recreate is for.
    pub fn refresh_document(
        &self,
        loaded: &LoadedVault,
        id: &kataan_core::id::CanonicalId,
    ) -> Result<bool> {
        let mut connection = self.open_connection()?;
        let transaction = connection.transaction()?;

        // "Has this index ever been built?", not "does the file exist". Opening
        // a connection *creates* the file and its empty schema — `search()`
        // does it on the first query — so `exists()` was true long before there
        // was anything in it. A write would then amend an empty index and
        // report success, leaving the vault with exactly one searchable
        // document and nothing to ever trigger a rebuild.
        if read_last_indexed_at(&transaction)?.is_none() {
            return Ok(false);
        }

        // Both spellings, always. A document that changed folder status is
        // indexed under the *other* one, and the key derived from the current
        // record can never be that one — so a conditional second delete could
        // not do this job.
        for kind in [Kind::Document, Kind::Folder] {
            delete_item(&transaction, &kind.item_key(id.as_str()))?;
        }

        let Some(record) = loaded.documents.get(id) else {
            // Gone from the vault: the deletes above are the whole update.
            transaction.commit()?;
            return Ok(true);
        };

        let markdown = loaded
            .read_markdown(id)
            .with_context(|| format!("failed to read markdown for `{id}`"))?;
        let item = SearchItem::from_document_record(loaded, record, &markdown)?;

        if let Err(error) = insert_item(&transaction, &item) {
            // Most likely an index built on an older schema, whose columns this
            // insert does not match — the caller falls back to a full rebuild.
            // The transaction rolls back on drop, so nothing is half-written.
            tracing::debug!(error = %error, "incremental search update failed; rebuilding");
            return Ok(false);
        }

        // Kept truthful: every write now takes this path, so leaving the marker
        // to `reindex_loaded` alone would make a current index report an
        // ever-staler timestamp — and it is the marker the check above reads.
        set_last_indexed_at(&transaction, &kataan_core::time::unix_timestamp_string())?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        let connection = self.connect()?;
        let raw_query = query.q.as_deref().unwrap_or_default().trim().to_owned();
        let fts_query = fts_query_for(&raw_query);
        let limit = query.limit.unwrap_or(20).clamp(1, 100);
        // Bound it: `offset as i64` above i64::MAX goes negative, and SQLite
        // reads a negative OFFSET as 0 — so a client with an arithmetic bug
        // silently re-reads the first page forever.
        let offset = query.offset.unwrap_or(0).min(i64::MAX as usize);
        let filters = SearchFilters::from_query(query);

        let rows = match (&fts_query, raw_query.is_empty()) {
            (Some(fts_query), _) => search_fts(&connection, fts_query, &filters, limit, offset)?,
            // No query text at all: this is a filtered listing.
            (None, true) => search_filtered(&connection, &filters, limit, offset)?,
            // Query text was given but nothing searchable survived tokenising
            // (`C++`, `&&`, an emoji). Falling through to the listing path
            // would present the whole vault as keyword matches.
            (None, false) => Vec::new(),
        };

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let facets = facets_for_item(&connection, &row.item_key)?;
            results.push(row.into_result(facets));
        }

        // Counted over the whole filtered match set, not the page just
        // returned. Counting the page made every number a function of `limit`
        // and hid any facet with no hit on the current page — so the one thing
        // facets are for, seeing what is available to narrow by, did not work.
        let facets = match (&fts_query, raw_query.is_empty()) {
            (Some(fts_query), _) => facet_counts_fts(&connection, fts_query, &filters)?,
            (None, true) => facet_counts_filtered(&connection, &filters)?,
            // Query text that tokenised to nothing matches nothing, so there is
            // nothing to narrow.
            (None, false) => Vec::new(),
        };

        Ok(SearchResponse {
            query: raw_query,
            mode: "keyword".to_owned(),
            facets,
            results,
        })
    }

    pub fn status(&self) -> Result<SearchStatus> {
        let connection = self.connect()?;
        let counts = count_by_kind(&connection)?;
        let count_of = |kind: &str| counts.get(kind).copied().unwrap_or(0);

        Ok(SearchStatus {
            index_path: self.path.display().to_string(),
            exists: true,
            item_count: counts.values().sum(),
            document_count: count_of(Kind::Document.as_str()),
            folder_count: count_of(Kind::Folder.as_str()),
            last_indexed_at: metadata_value(&connection, "last_indexed_at")?,
        })
    }

    /// Open the SQLite file (creating its directory) without touching the
    /// schema. `reindex_loaded` uses this because it rebuilds the schema itself.
    fn open_connection(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create search index directory `{}`",
                    parent.display()
                )
            })?;
        }
        let connection = Connection::open(&self.path)
            .with_context(|| format!("failed to open search index `{}`", self.path.display()))?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    fn connect(&self) -> Result<Connection> {
        let connection = self.open_connection()?;
        create_schema(&connection)?;
        Ok(connection)
    }
}

mod sql;

use sql::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Folder,
    Document,
}

impl Kind {
    /// The index's primary key for `id`. Spelled in one place: a delete built
    /// with a drifted separator matches nothing, and the stale row survives
    /// while the index reports success.
    fn item_key(self, id: &str) -> String {
        format!("{}:{id}", self.as_str())
    }

    fn as_str(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Document => "document",
        }
    }
}

#[derive(Debug, Clone)]
struct SearchItem {
    item_key: String,
    kind: Kind,
    id: Option<String>,
    path: String,
    title: Option<String>,
    type_name: Option<String>,
    status: Option<String>,
    extension: Option<String>,
    aliases: String,
    facets: Vec<String>,
    metadata: String,
    body: String,
}

impl SearchItem {
    fn from_document_record(
        loaded: &LoadedVault,
        record: &DocumentRecord,
        markdown: &str,
    ) -> Result<Self> {
        let kind = if record.is_folder_index {
            Kind::Folder
        } else {
            Kind::Document
        };
        let id = record.id.as_str().to_owned();
        let path = kataan_core::walk::relative_slug(&loaded.root, &record.markdown_path);
        let title = document_title(record, markdown);
        let extension = record
            .markdown_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_owned);
        let aliases = record.metadata.aliases.join(" ");
        let metadata = metadata_text(record);

        Ok(Self {
            item_key: kind.item_key(&id),
            kind,
            id: Some(id),
            path,
            title,
            type_name: Some(record.metadata.r#type.clone()),
            status: record.metadata.status.clone(),
            extension,
            aliases,
            facets: record.facets.clone(),
            metadata,
            body: markdown.to_owned(),
        })
    }
}

pub fn default_index_path(vault_root: &Path) -> PathBuf {
    let root = vault_root
        .canonicalize()
        .unwrap_or_else(|_| vault_root.to_path_buf());
    let hash = blake3::hash(root.to_string_lossy().as_bytes());
    cache_base_dir()
        .join("kataan")
        .join("search")
        .join(&hash.to_hex()[..16])
        .join("search.sqlite")
}

fn cache_base_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    std::env::temp_dir()
}

fn fts_query_for(query: &str) -> Option<String> {
    let terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.trim().is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();

    (!terms.is_empty()).then(|| terms.join(" "))
}

fn blank_as_none(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

fn document_title(record: &DocumentRecord, markdown: &str) -> Option<String> {
    first_markdown_heading(markdown)
        .or_else(|| kataan_core::document::display_name(&record.metadata))
        .or_else(|| Some(title_from_id(record.id.as_str())))
}

fn first_markdown_heading(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return None;
        }
        let title = trimmed.trim_start_matches('#').trim();
        (!title.is_empty()).then(|| title.to_owned())
    })
}

fn metadata_text(record: &DocumentRecord) -> String {
    let mut parts = vec![
        record.metadata.r#type.clone(),
        record.metadata.markdown.clone(),
    ];

    if let Some(status) = &record.metadata.status {
        parts.push(status.clone());
    }
    if let Some(created_by) = &record.metadata.created_by {
        parts.push(created_by.clone());
    }
    if let Some(last_updated_by) = &record.metadata.last_updated_by {
        parts.push(last_updated_by.clone());
    }

    for alias in &record.metadata.aliases {
        parts.push(alias.clone());
    }
    for label in &record.metadata.labels {
        parts.push(label.clone());
    }
    for ancestor in &record.ancestors {
        parts.push(ancestor.clone());
    }
    for facet in &record.facets {
        parts.push(facet.clone());
    }
    for (predicate, targets) in &record.metadata.edges {
        parts.push(predicate.clone());
        parts.extend(targets.iter().cloned());
    }

    dedupe_preserve_order(parts).join(" ")
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if !value.trim().is_empty() && seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

#[cfg(test)]
mod tests;
