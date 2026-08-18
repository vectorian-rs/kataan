use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use kataan_core::{
    title::title_from_id,
    vault::{route_token_for_id, DocumentRecord, LoadedVault},
};
use rusqlite::{named_params, params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub const EXTRACTOR_VERSION: &str = "search-v1";

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
    pub route_token: Option<String>,
    pub facets: Vec<String>,
    pub snippet: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub extractor_version: Option<String>,
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
        let connection = Connection::open(&path)
            .with_context(|| format!("failed to open search index `{}`", path.display()))?;
        create_schema(&connection)?;
        Ok(Self { path })
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
                extractor_version: None,
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
            "INSERT OR REPLACE INTO search_metadata(key, value) VALUES (?1, ?2), (?3, ?4)",
            params![
                "extractor_version",
                EXTRACTOR_VERSION,
                "last_indexed_at",
                indexed_at,
            ],
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

    pub fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        let connection = self.connect()?;
        let raw_query = query.q.as_deref().unwrap_or_default().trim().to_owned();
        let fts_query = fts_query_for(&raw_query);
        let limit = query.limit.unwrap_or(20).clamp(1, 100);
        let offset = query.offset.unwrap_or(0);
        let filters = SearchFilters::from_query(query);

        let rows = if let Some(fts_query) = fts_query {
            search_fts(&connection, &fts_query, &filters, limit, offset)?
        } else {
            search_filtered(&connection, &filters, limit, offset)?
        };

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let facets = facets_for_item(&connection, &row.item_key)?;
            results.push(row.into_result(facets));
        }

        Ok(SearchResponse {
            query: raw_query,
            mode: "keyword".to_owned(),
            facets: facet_counts(&results),
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
            extractor_version: metadata_value(&connection, "extractor_version")?,
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
        Connection::open(&self.path)
            .with_context(|| format!("failed to open search index `{}`", self.path.display()))
    }

    fn connect(&self) -> Result<Connection> {
        let connection = self.open_connection()?;
        create_schema(&connection)?;
        Ok(connection)
    }
}

/// The kind of thing an index item represents. Serializes to the exact wire
/// strings the API and web UI depend on (`"folder"`/`"document"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Folder,
    Document,
}

impl Kind {
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
    route_token: Option<String>,
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
            item_key: format!("{}:{id}", kind.as_str()),
            kind,
            id: Some(id),
            path,
            title,
            type_name: Some(record.metadata.r#type.clone()),
            status: record.metadata.status.clone(),
            extension,
            route_token: Some(route_token_for_id(&record.id)),
            aliases,
            facets: record.facets.clone(),
            metadata,
            body: markdown.to_owned(),
        })
    }
}

#[derive(Debug, Clone)]
struct SearchRow {
    item_key: String,
    kind: String,
    id: Option<String>,
    path: String,
    title: Option<String>,
    type_name: Option<String>,
    status: Option<String>,
    extension: Option<String>,
    route_token: Option<String>,
    snippet: Option<String>,
    score: f64,
}

impl SearchRow {
    fn into_result(self, facets: Vec<String>) -> SearchResult {
        SearchResult {
            kind: self.kind,
            id: self.id,
            path: self.path,
            title: self.title,
            r#type: self.type_name,
            status: self.status,
            extension: self.extension,
            route_token: self.route_token,
            facets,
            snippet: self.snippet,
            score: self.score,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SearchFilters<'a> {
    kind: Option<&'a str>,
    type_filter: Option<&'a str>,
    status: Option<&'a str>,
    facet: Option<&'a str>,
    path_prefix: Option<&'a str>,
}

impl<'a> SearchFilters<'a> {
    fn from_query(query: &'a SearchQuery) -> Self {
        Self {
            kind: blank_as_none(query.kind.as_deref()),
            type_filter: blank_as_none(query.type_filter.as_deref()),
            status: blank_as_none(query.status.as_deref()),
            facet: blank_as_none(query.facet.as_deref()),
            path_prefix: blank_as_none(query.path_prefix.as_deref()),
        }
    }
}

const SEARCH_FILTER_SQL: &str = "\
           AND (:kind IS NULL OR i.kind = :kind)
           AND (:type_filter IS NULL OR i.type = :type_filter)
           AND (:status IS NULL OR i.status = :status)
           AND (:facet IS NULL OR EXISTS (
             SELECT 1 FROM search_facets sf
             WHERE sf.item_key = i.item_key AND sf.facet = :facet
           ))
           AND (
             :path_prefix IS NULL
             OR i.path = :path_prefix
             OR i.path LIKE (:path_prefix || '/%')
           )";

fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS search_items (
           item_key TEXT PRIMARY KEY,
           kind TEXT NOT NULL,
           id TEXT,
           path TEXT NOT NULL,
           title TEXT,
           type TEXT,
           status TEXT,
           extension TEXT,
           route_token TEXT
         );

         CREATE TABLE IF NOT EXISTS search_facets (
           item_key TEXT NOT NULL,
           facet TEXT NOT NULL,
           PRIMARY KEY (item_key, facet)
         );

         CREATE INDEX IF NOT EXISTS search_items_kind_idx ON search_items(kind);
         CREATE INDEX IF NOT EXISTS search_items_type_idx ON search_items(type);
         CREATE INDEX IF NOT EXISTS search_items_status_idx ON search_items(status);
         CREATE INDEX IF NOT EXISTS search_items_path_idx ON search_items(path);
         CREATE INDEX IF NOT EXISTS search_facets_facet_idx ON search_facets(facet);

         CREATE TABLE IF NOT EXISTS search_metadata (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );

         CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
           item_key UNINDEXED,
           title,
           path,
           aliases,
           facets,
           metadata,
           body,
           tokenize = 'unicode61 remove_diacritics 2'
         );",
    )?;
    Ok(())
}

fn insert_item(connection: &Connection, item: &SearchItem) -> Result<()> {
    connection
        .prepare_cached(
            "INSERT INTO search_items(
               item_key, kind, id, path, title, type, status, extension, route_token
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?
        .execute(params![
            &item.item_key,
            item.kind.as_str(),
            item.id.as_deref(),
            &item.path,
            item.title.as_deref(),
            item.type_name.as_deref(),
            item.status.as_deref(),
            item.extension.as_deref(),
            item.route_token.as_deref(),
        ])?;

    let facet_text = item.facets.join(" ");
    connection
        .prepare_cached(
            "INSERT INTO search_fts(item_key, title, path, aliases, facets, metadata, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            &item.item_key,
            item.title.as_deref(),
            &item.path,
            &item.aliases,
            &facet_text,
            &item.metadata,
            &item.body,
        ])?;

    let mut facet_statement = connection
        .prepare_cached("INSERT OR IGNORE INTO search_facets(item_key, facet) VALUES (?1, ?2)")?;
    for facet in &item.facets {
        facet_statement.execute(params![&item.item_key, facet])?;
    }

    Ok(())
}

fn search_fts(
    connection: &Connection,
    fts_query: &str,
    filters: &SearchFilters<'_>,
    limit: usize,
    offset: usize,
) -> Result<Vec<SearchRow>> {
    let sql = format!(
        "SELECT
           i.item_key,
           i.kind,
           i.id,
           i.path,
           i.title,
           i.type,
           i.status,
           i.extension,
           i.route_token,
           snippet(search_fts, -1, '<mark>', '</mark>', '…', 24) AS snippet,
           bm25(search_fts, 0.0, 5.0, 3.0, 4.0, 3.0, 2.0, 1.0) AS rank
         FROM search_fts
         JOIN search_items i ON i.item_key = search_fts.item_key
         WHERE search_fts MATCH :fts_query
{SEARCH_FILTER_SQL}
         ORDER BY rank ASC, i.path ASC
         LIMIT :limit OFFSET :offset"
    );
    let mut statement = connection.prepare(&sql)?;

    let rows = statement.query_map(
        named_params! {
            ":fts_query": fts_query,
            ":kind": filters.kind,
            ":type_filter": filters.type_filter,
            ":status": filters.status,
            ":facet": filters.facet,
            ":path_prefix": filters.path_prefix,
            ":limit": limit as i64,
            ":offset": offset as i64,
        },
        search_row_from_fts,
    )?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn search_filtered(
    connection: &Connection,
    filters: &SearchFilters<'_>,
    limit: usize,
    offset: usize,
) -> Result<Vec<SearchRow>> {
    let sql = format!(
        "SELECT
           i.item_key,
           i.kind,
           i.id,
           i.path,
           i.title,
           i.type,
           i.status,
           i.extension,
           i.route_token
         FROM search_items i
         WHERE 1 = 1
{SEARCH_FILTER_SQL}
         ORDER BY COALESCE(i.title, i.path) ASC, i.path ASC
         LIMIT :limit OFFSET :offset"
    );
    let mut statement = connection.prepare(&sql)?;

    let rows = statement.query_map(
        named_params! {
            ":kind": filters.kind,
            ":type_filter": filters.type_filter,
            ":status": filters.status,
            ":facet": filters.facet,
            ":path_prefix": filters.path_prefix,
            ":limit": limit as i64,
            ":offset": offset as i64,
        },
        search_row_without_snippet,
    )?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn search_row_from_fts(row: &Row<'_>) -> rusqlite::Result<SearchRow> {
    let snippet = row.get(9)?;
    let rank: f64 = row.get(10)?;
    search_row(row, snippet, -rank)
}

fn search_row_without_snippet(row: &Row<'_>) -> rusqlite::Result<SearchRow> {
    search_row(row, None, 0.0)
}

fn search_row(row: &Row<'_>, snippet: Option<String>, score: f64) -> rusqlite::Result<SearchRow> {
    Ok(SearchRow {
        item_key: row.get(0)?,
        kind: row.get(1)?,
        id: row.get(2)?,
        path: row.get(3)?,
        title: row.get(4)?,
        type_name: row.get(5)?,
        status: row.get(6)?,
        extension: row.get(7)?,
        route_token: row.get(8)?,
        snippet,
        score,
    })
}

fn facets_for_item(connection: &Connection, item_key: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare_cached("SELECT facet FROM search_facets WHERE item_key = ?1 ORDER BY facet ASC")?;
    let facets = statement
        .query_map(params![item_key], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(facets)
}

fn facet_counts(results: &[SearchResult]) -> Vec<SearchFacetCount> {
    let mut counts = BTreeMap::<String, usize>::new();
    for result in results {
        for facet in &result.facets {
            *counts.entry(facet.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(facet, count)| SearchFacetCount { facet, count })
        .collect()
}

fn count_by_kind(connection: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut statement =
        connection.prepare("SELECT kind, COUNT(*) FROM search_items GROUP BY kind")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let (kind, count) = row?;
        counts.insert(kind, count);
    }
    Ok(counts)
}

fn metadata_value(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM search_metadata WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
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
