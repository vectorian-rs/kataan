//! The SQLite layer: schema, statements, and the row shapes they produce.
//!
//! Kept apart from `SearchIndex` so the policy — what a query means, when the
//! index is stale, what a write refreshes — reads without the SQL it is
//! expressed in, and so the FTS and non-FTS variants of each query sit next to
//! each other where their difference is visible.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::Result;

use crate::{blank_as_none, SearchFacetCount, SearchItem, SearchQuery, SearchResult};

/// Pragmas for every freshly-opened connection: WAL so readers never block on
/// the reindex writer, and a busy timeout so concurrent writers wait-and-retry
/// instead of failing immediately with `SQLITE_BUSY`.
pub(crate) fn configure_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.query_row("PRAGMA journal_mode = WAL;", [], |_row| Ok(()))?;
    Ok(())
}

/// The kind of thing an index item represents. Serializes to the exact wire
/// strings the API and web UI depend on (`"folder"`/`"document"`).

#[derive(Debug, Clone)]
pub(crate) struct SearchRow {
    pub(crate) item_key: String,
    pub(crate) kind: String,
    pub(crate) id: Option<String>,
    pub(crate) path: String,
    pub(crate) title: Option<String>,
    pub(crate) type_name: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) extension: Option<String>,
    pub(crate) snippet: Option<String>,
    pub(crate) score: f64,
}

impl SearchRow {
    pub(crate) fn into_result(self, facets: Vec<String>) -> SearchResult {
        SearchResult {
            kind: self.kind,
            id: self.id,
            path: self.path,
            title: self.title,
            r#type: self.type_name,
            status: self.status,
            extension: self.extension,
            facets,
            snippet: self.snippet,
            score: self.score,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SearchFilters<'a> {
    pub(crate) kind: Option<&'a str>,
    pub(crate) type_filter: Option<&'a str>,
    pub(crate) status: Option<&'a str>,
    pub(crate) facet: Option<&'a str>,
    pub(crate) path_prefix: Option<&'a str>,
}

impl<'a> SearchFilters<'a> {
    /// The bindings for [`SEARCH_FILTER_SQL`], in one place.
    ///
    /// Four queries embed that clause — the two result queries and the two
    /// facet-count queries — and each was restating the same five bindings.
    /// Adding a filter meant editing five places, four of them silently
    /// optional.
    pub(crate) fn params(&self) -> Vec<(&'static str, &dyn rusqlite::ToSql)> {
        vec![
            (":kind", &self.kind),
            (":type_filter", &self.type_filter),
            (":status", &self.status),
            (":facet", &self.facet),
            (":path_prefix", &self.path_prefix),
        ]
    }

    pub(crate) fn from_query(query: &'a SearchQuery) -> Self {
        Self {
            kind: blank_as_none(query.kind.as_deref()),
            type_filter: blank_as_none(query.type_filter.as_deref()),
            status: blank_as_none(query.status.as_deref()),
            facet: blank_as_none(query.facet.as_deref()),
            path_prefix: blank_as_none(query.path_prefix.as_deref()),
        }
    }
}

pub(crate) const SEARCH_FILTER_SQL: &str = "\
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

pub(crate) fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS search_items (
           item_key TEXT PRIMARY KEY,
           kind TEXT NOT NULL,
           id TEXT,
           path TEXT NOT NULL,
           title TEXT,
           type TEXT,
           status TEXT,
           extension TEXT
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

/// Remove every row for one `item_key`, across all three tables that carry it.
/// `last_indexed_at`, or `None` when the index has never been built.
pub(crate) fn read_last_indexed_at(connection: &Connection) -> Result<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT value FROM search_metadata WHERE key = 'last_indexed_at'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

pub(crate) fn set_last_indexed_at(connection: &Connection, indexed_at: &str) -> Result<()> {
    connection.execute(
        "INSERT OR REPLACE INTO search_metadata(key, value) VALUES ('last_indexed_at', ?1)",
        params![indexed_at],
    )?;
    Ok(())
}

pub(crate) fn delete_item(connection: &Connection, item_key: &str) -> Result<()> {
    for statement in [
        "DELETE FROM search_fts WHERE item_key = ?1",
        "DELETE FROM search_facets WHERE item_key = ?1",
        "DELETE FROM search_items WHERE item_key = ?1",
    ] {
        connection
            .prepare_cached(statement)?
            .execute(params![item_key])?;
    }
    Ok(())
}

pub(crate) fn insert_item(connection: &Connection, item: &SearchItem) -> Result<()> {
    connection
        .prepare_cached(
            "INSERT INTO search_items(
               item_key, kind, id, path, title, type, status, extension
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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

pub(crate) fn search_fts(
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

    let (limit, offset) = (limit as i64, offset as i64);
    let mut params = filters.params();
    params.push((":fts_query", &fts_query));
    params.push((":limit", &limit));
    params.push((":offset", &offset));

    let rows = statement.query_map(params.as_slice(), search_row_from_fts)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(crate) fn search_filtered(
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
           i.extension
         FROM search_items i
         WHERE 1 = 1
{SEARCH_FILTER_SQL}
         ORDER BY COALESCE(i.title, i.path) ASC, i.path ASC
         LIMIT :limit OFFSET :offset"
    );
    let mut statement = connection.prepare(&sql)?;

    let (limit, offset) = (limit as i64, offset as i64);
    let mut params = filters.params();
    params.push((":limit", &limit));
    params.push((":offset", &offset));

    let rows = statement.query_map(params.as_slice(), search_row_without_snippet)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(crate) fn search_row_from_fts(row: &Row<'_>) -> rusqlite::Result<SearchRow> {
    let snippet = row.get(8)?;
    let rank: f64 = row.get(9)?;
    search_row(row, snippet, -rank)
}

pub(crate) fn search_row_without_snippet(row: &Row<'_>) -> rusqlite::Result<SearchRow> {
    search_row(row, None, 0.0)
}

pub(crate) fn search_row(
    row: &Row<'_>,
    snippet: Option<String>,
    score: f64,
) -> rusqlite::Result<SearchRow> {
    Ok(SearchRow {
        item_key: row.get(0)?,
        kind: row.get(1)?,
        id: row.get(2)?,
        path: row.get(3)?,
        title: row.get(4)?,
        type_name: row.get(5)?,
        status: row.get(6)?,
        extension: row.get(7)?,
        snippet,
        score,
    })
}

pub(crate) fn facets_for_item(connection: &Connection, item_key: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare_cached("SELECT facet FROM search_facets WHERE item_key = ?1 ORDER BY facet ASC")?;
    let facets = statement
        .query_map(params![item_key], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(facets)
}

/// Facet counts over every document the query matches, for the keyword path.
pub(crate) fn facet_counts_fts(
    connection: &Connection,
    fts_query: &str,
    filters: &SearchFilters<'_>,
) -> Result<Vec<SearchFacetCount>> {
    let sql = format!(
        "SELECT f.facet, COUNT(*) AS hits
         FROM search_fts
         JOIN search_items i ON i.item_key = search_fts.item_key
         JOIN search_facets f ON f.item_key = i.item_key
         WHERE search_fts MATCH :fts_query
{SEARCH_FILTER_SQL}
         GROUP BY f.facet
         ORDER BY hits DESC, f.facet ASC"
    );
    let mut params = filters.params();
    params.push((":fts_query", &fts_query));
    collect_facet_counts(connection, &sql, params.as_slice())
}

/// The same, for a filtered listing with no query text.
pub(crate) fn facet_counts_filtered(
    connection: &Connection,
    filters: &SearchFilters<'_>,
) -> Result<Vec<SearchFacetCount>> {
    let sql = format!(
        "SELECT f.facet, COUNT(*) AS hits
         FROM search_items i
         JOIN search_facets f ON f.item_key = i.item_key
         WHERE 1 = 1
{SEARCH_FILTER_SQL}
         GROUP BY f.facet
         ORDER BY hits DESC, f.facet ASC"
    );
    collect_facet_counts(connection, &sql, filters.params().as_slice())
}

pub(crate) fn collect_facet_counts(
    connection: &Connection,
    sql: &str,
    params: &[(&str, &dyn rusqlite::ToSql)],
) -> Result<Vec<SearchFacetCount>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params, |row| {
        Ok(SearchFacetCount {
            facet: row.get(0)?,
            count: row.get::<_, i64>(1)? as usize,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn count_by_kind(connection: &Connection) -> Result<BTreeMap<String, usize>> {
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

pub(crate) fn metadata_value(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM search_metadata WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}
