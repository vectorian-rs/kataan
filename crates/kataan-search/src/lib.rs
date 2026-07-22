use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use kataan_core::vault::{route_token_for_id, DocumentRecord, LoadedVault};
use rusqlite::{params, Connection, OptionalExtension};
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
    pub file_count: usize,
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
    pub file_count: usize,
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
                file_count: 0,
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
        let mut connection = self.connect()?;
        let indexed_at = unix_timestamp_string()?;
        let transaction = connection.transaction()?;

        transaction.execute_batch(
            "DELETE FROM search_fts;
             DELETE FROM search_facets;
             DELETE FROM search_items;
             DELETE FROM search_metadata;",
        )?;

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
            if item.kind == "folder" {
                folder_count += 1;
            } else if item.kind == "document" {
                document_count += 1;
            }
        }

        transaction.execute(
            "INSERT OR REPLACE INTO search_metadata(key, value) VALUES (?1, ?2)",
            params!["extractor_version", EXTRACTOR_VERSION],
        )?;
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
            file_count: 0,
            indexed_at,
        })
    }

    pub fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        let connection = self.connect()?;
        let raw_query = query.q.as_deref().unwrap_or_default().trim().to_owned();
        let fts_query = fts_query_for(&raw_query);
        let limit = query.limit.unwrap_or(20).clamp(1, 100);
        let offset = query.offset.unwrap_or(0);
        let kind = blank_as_none(query.kind.as_deref());
        let type_filter = blank_as_none(query.type_filter.as_deref());
        let status = blank_as_none(query.status.as_deref());
        let facet = blank_as_none(query.facet.as_deref());
        let path_prefix = blank_as_none(query.path_prefix.as_deref());

        let rows = if let Some(fts_query) = fts_query {
            search_fts(
                &connection,
                &fts_query,
                kind,
                type_filter,
                status,
                facet,
                path_prefix,
                limit,
                offset,
            )?
        } else {
            search_filtered(
                &connection,
                kind,
                type_filter,
                status,
                facet,
                path_prefix,
                limit,
                offset,
            )?
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
        let item_count = count_items(&connection, None)?;
        let document_count = count_items(&connection, Some("document"))?;
        let folder_count = count_items(&connection, Some("folder"))?;
        let file_count = count_items(&connection, Some("file"))?;

        Ok(SearchStatus {
            index_path: self.path.display().to_string(),
            exists: true,
            item_count,
            document_count,
            folder_count,
            file_count,
            last_indexed_at: metadata_value(&connection, "last_indexed_at")?,
            extractor_version: metadata_value(&connection, "extractor_version")?,
        })
    }

    fn connect(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)
            .with_context(|| format!("failed to open search index `{}`", self.path.display()))?;
        create_schema(&connection)?;
        Ok(connection)
    }
}

#[derive(Debug, Clone)]
struct SearchItem {
    item_key: String,
    kind: String,
    id: Option<String>,
    path: String,
    title: Option<String>,
    type_name: Option<String>,
    status: Option<String>,
    extension: Option<String>,
    route_token: Option<String>,
    checksum: String,
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
            "folder"
        } else {
            "document"
        }
        .to_owned();
        let id = record.id.as_str().to_owned();
        let path = relative_path(&loaded.root, &record.markdown_path);
        let title = document_title(record, markdown);
        let extension = record
            .markdown_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_owned);
        let checksum = format!(
            "markdown={} toml={}",
            record.markdown_checksum.as_deref().unwrap_or_default(),
            record.toml_checksum
        );
        let aliases = record.metadata.aliases.join(" ");
        let metadata = metadata_text(record);

        Ok(Self {
            item_key: format!("{kind}:{id}"),
            kind,
            id: Some(id),
            path,
            title,
            type_name: Some(record.metadata.r#type.clone()),
            status: record.metadata.status.clone(),
            extension,
            route_token: Some(route_token_for_id(&record.id)),
            checksum,
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
           route_token TEXT,
           checksum TEXT NOT NULL,
           extractor_version TEXT NOT NULL,
           indexed_at TEXT NOT NULL
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
    let indexed_at = unix_timestamp_string()?;
    connection.execute(
        "INSERT INTO search_items(
           item_key, kind, id, path, title, type, status, extension, route_token,
           checksum, extractor_version, indexed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            &item.item_key,
            &item.kind,
            item.id.as_deref(),
            &item.path,
            item.title.as_deref(),
            item.type_name.as_deref(),
            item.status.as_deref(),
            item.extension.as_deref(),
            item.route_token.as_deref(),
            &item.checksum,
            EXTRACTOR_VERSION,
            &indexed_at,
        ],
    )?;

    let facet_text = item.facets.join(" ");
    connection.execute(
        "INSERT INTO search_fts(item_key, title, path, aliases, facets, metadata, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &item.item_key,
            item.title.as_deref(),
            &item.path,
            &item.aliases,
            &facet_text,
            &item.metadata,
            &item.body,
        ],
    )?;

    for facet in &item.facets {
        connection.execute(
            "INSERT OR IGNORE INTO search_facets(item_key, facet) VALUES (?1, ?2)",
            params![&item.item_key, facet],
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn search_fts(
    connection: &Connection,
    fts_query: &str,
    kind: Option<&str>,
    type_filter: Option<&str>,
    status: Option<&str>,
    facet: Option<&str>,
    path_prefix: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<SearchRow>> {
    let mut statement = connection.prepare(
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
         WHERE search_fts MATCH ?1
           AND (?2 IS NULL OR i.kind = ?2)
           AND (?3 IS NULL OR i.type = ?3)
           AND (?4 IS NULL OR i.status = ?4)
           AND (?5 IS NULL OR EXISTS (
             SELECT 1 FROM search_facets sf
             WHERE sf.item_key = i.item_key AND sf.facet = ?5
           ))
           AND (?6 IS NULL OR i.path = ?6 OR i.path LIKE (?6 || '%'))
         ORDER BY rank ASC, i.path ASC
         LIMIT ?7 OFFSET ?8",
    )?;

    let rows = statement.query_map(
        params![
            fts_query,
            kind,
            type_filter,
            status,
            facet,
            path_prefix,
            limit as i64,
            offset as i64,
        ],
        |row| {
            let rank: f64 = row.get(10)?;
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
                snippet: row.get(9)?,
                score: -rank,
            })
        },
    )?;

    collect_rows(rows)
}

#[allow(clippy::too_many_arguments)]
fn search_filtered(
    connection: &Connection,
    kind: Option<&str>,
    type_filter: Option<&str>,
    status: Option<&str>,
    facet: Option<&str>,
    path_prefix: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<SearchRow>> {
    let mut statement = connection.prepare(
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
         WHERE (?1 IS NULL OR i.kind = ?1)
           AND (?2 IS NULL OR i.type = ?2)
           AND (?3 IS NULL OR i.status = ?3)
           AND (?4 IS NULL OR EXISTS (
             SELECT 1 FROM search_facets sf
             WHERE sf.item_key = i.item_key AND sf.facet = ?4
           ))
           AND (?5 IS NULL OR i.path = ?5 OR i.path LIKE (?5 || '%'))
         ORDER BY COALESCE(i.title, i.path) ASC, i.path ASC
         LIMIT ?6 OFFSET ?7",
    )?;

    let rows = statement.query_map(
        params![
            kind,
            type_filter,
            status,
            facet,
            path_prefix,
            limit as i64,
            offset as i64,
        ],
        |row| {
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
                snippet: None,
                score: 0.0,
            })
        },
    )?;

    collect_rows(rows)
}

fn collect_rows(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<SearchRow>>,
) -> Result<Vec<SearchRow>> {
    let mut collected = Vec::new();
    for row in rows {
        collected.push(row?);
    }
    Ok(collected)
}

fn facets_for_item(connection: &Connection, item_key: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT facet FROM search_facets WHERE item_key = ?1 ORDER BY facet ASC")?;
    let rows = statement.query_map(params![item_key], |row| row.get::<_, String>(0))?;
    let mut facets = Vec::new();
    for row in rows {
        facets.push(row?);
    }
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

fn count_items(connection: &Connection, kind: Option<&str>) -> Result<usize> {
    let count: i64 = if let Some(kind) = kind {
        connection.query_row(
            "SELECT COUNT(*) FROM search_items WHERE kind = ?1",
            params![kind],
            |row| row.get(0),
        )?
    } else {
        connection.query_row("SELECT COUNT(*) FROM search_items", [], |row| row.get(0))?
    };
    Ok(count as usize)
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
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn document_title(record: &DocumentRecord, markdown: &str) -> Option<String> {
    first_markdown_heading(markdown)
        .or_else(|| record.metadata.aliases.first().cloned())
        .or_else(|| record.metadata.labels.first().cloned())
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

fn unix_timestamp_string() -> Result<String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn indexes_and_searches_documents() {
        let root = temp_dir("documents");
        kataan_core::init::init_vault(&root, "Search Test").unwrap();
        write_note(
            &root,
            "session-store",
            "# Session Store\n\nThe durable session store coordinates search state.",
            r#"type = "note"
status = "active"
markdown = "session-store.md"
aliases = ["session cache"]
labels = ["rust", "local-first"]
"#,
        );

        let loaded = LoadedVault::load(&root).unwrap();
        let db_path = root.join("search.sqlite");
        let index = SearchIndex::open(&db_path).unwrap();
        let response = index.reindex_loaded(&loaded).unwrap();
        assert!(response.document_count > 0);

        let response = index
            .search(&SearchQuery {
                q: Some("durable session".to_owned()),
                ..SearchQuery::default()
            })
            .unwrap();
        assert!(response
            .results
            .iter()
            .any(|result| result.id.as_deref() == Some("notes/session-store")));

        let folder_response = index
            .search(&SearchQuery {
                q: Some("Projects".to_owned()),
                kind: Some("folder".to_owned()),
                ..SearchQuery::default()
            })
            .unwrap();
        assert!(folder_response
            .results
            .iter()
            .any(|result| result.id.as_deref() == Some("projects")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn searches_aliases_facets_type_status_and_path() {
        let root = temp_dir("filters");
        kataan_core::init::init_vault(&root, "Search Test").unwrap();
        fs::create_dir_all(root.join("projects/company-x")).unwrap();
        fs::write(
            root.join("projects/company-x/q2-launch.md"),
            "# Q2 Launch\n\nLaunch notes.",
        )
        .unwrap();
        fs::write(
            root.join("projects/company-x/q2-launch.toml"),
            r#"type = "project"
status = "active"
markdown = "q2-launch.md"
aliases = ["rocket plan"]
labels = ["launch"]
"#,
        )
        .unwrap();

        let loaded = LoadedVault::load(&root).unwrap();
        let index = SearchIndex::open(root.join("search.sqlite")).unwrap();
        index.reindex_loaded(&loaded).unwrap();

        let alias_response = index
            .search(&SearchQuery {
                q: Some("rocket".to_owned()),
                ..SearchQuery::default()
            })
            .unwrap();
        assert_eq!(
            alias_response.results[0].id.as_deref(),
            Some("projects/company-x/q2-launch")
        );

        let filtered_response = index
            .search(&SearchQuery {
                q: Some("launch".to_owned()),
                kind: Some("document".to_owned()),
                type_filter: Some("project".to_owned()),
                status: Some("active".to_owned()),
                facet: Some("company-x".to_owned()),
                path_prefix: Some("projects/company-x".to_owned()),
                ..SearchQuery::default()
            })
            .unwrap();
        assert!(filtered_response
            .results
            .iter()
            .any(|result| result.id.as_deref() == Some("projects/company-x/q2-launch")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn status_reports_missing_and_indexed_database() {
        let root = temp_dir("status");
        kataan_core::init::init_vault(&root, "Search Test").unwrap();
        let db_path = root.join("search.sqlite");

        let missing = SearchIndex::status_at_path(&db_path).unwrap();
        assert!(!missing.exists);

        let loaded = LoadedVault::load(&root).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        index.reindex_loaded(&loaded).unwrap();
        let status = index.status().unwrap();
        assert!(status.exists);
        assert!(status.item_count > 0);
        assert_eq!(status.extractor_version.as_deref(), Some(EXTRACTOR_VERSION));

        fs::remove_dir_all(root).unwrap();
    }

    fn write_note(root: &Path, slug: &str, markdown: &str, metadata: &str) {
        fs::write(root.join("notes").join(format!("{slug}.md")), markdown).unwrap();
        fs::write(root.join("notes").join(format!("{slug}.toml")), metadata).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("kataan-search-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
