# Kataan Search Plan

## Summary

Kataan search should start simple: a local, derived SQLite index using FTS5 for keyword search over vault documents and lightweight artifact metadata. PDFs are indexed by metadata only in v1 because they are usually generated outputs/artifacts, not source knowledge.

Semantic/vector search can be added later as an optional sidecar using USearch and Reciprocal Rank Fusion (RRF), but it should not be part of the first implementation.

## Goals

- Provide fast local search with no cloud dependency.
- Search Kataan Markdown documents, folder indexes, metadata, aliases, labels, type/status, and path ancestors.
- Search artifact filenames and lightweight metadata.
- Treat PDFs as artifacts and index metadata only by default.
- Keep the search index as a rebuildable cache, not vault truth.
- Support filters/facets for type, status, labels, ancestors, and result kind.
- Keep the first ranking model understandable: SQLite FTS5 BM25 plus explicit field weighting.

## Non-goals for v1

- No vector embeddings.
- No USearch index.
- No learned-to-rank model.
- No PDF full-text extraction or OCR.
- No indexing binary content beyond basic metadata.
- No remote search or hosted embedding APIs.

## Architecture

Add a dedicated search module/crate, preferably:

```txt
crates/kataan-search
```

Responsibilities:

- Build and update a local SQLite search database.
- Convert `LoadedVault` document records into searchable items.
- Walk artifact files while respecting Kataan ignore rules.
- Extract lightweight metadata for supported artifact types.
- Execute search queries and return ranked results/snippets/facets.

The server exposes this through API endpoints. The web UI consumes those endpoints.

The search index is derived cache data. It can be deleted and rebuilt at any time.

Recommended cache location:

```txt
$XDG_CACHE_HOME/kataan/search/<vault-hash>/search.sqlite
```

Do not store the search database in the vault by default.

## Indexed content

### Kataan documents

For Markdown+TOML document pairs and folder index documents, index:

- canonical ID
- route token
- Markdown body
- title/heading, where available
- aliases
- labels
- derived path ancestors
- type
- status
- TOML metadata fields that are useful for search
- outgoing edge target IDs and predicate names as metadata text

TOML sidecars should not appear as separate search results when they belong to a document. Their useful metadata is folded into the document result.

### Folder indexes

Folder `index.md` + `index.toml` pairs are searchable as folder results.

Index:

- folder canonical ID
- folder name/title
- folder Markdown body
- folder metadata
- derived ancestors/facets

### Artifacts/files

For non-document files, index metadata only unless the file is clearly text-like.

Text-like artifacts that may be full-text indexed in v1:

- standalone `.md`
- standalone `.txt`
- standalone `.toml`
- `.json`
- `.yaml` / `.yml`
- source files if desired later, but code search is not required for the first pass

For all artifacts, index:

- filename
- vault-relative path
- extension
- kind/media category
- containing folder ancestors
- size
- mtime
- checksum

### PDFs

PDFs are metadata-only in v1.

Index:

- filename
- vault-relative path
- containing folder ancestors
- extension/media type
- file size
- mtime
- checksum
- optional PDF document info metadata if available:
  - title
  - author
  - subject
  - keywords
  - page count

Do not extract PDF body text by default. Most Kataan PDFs are expected to be generated outputs, so full-text PDF indexing would duplicate source material and add noise.

Future optional config may enable PDF text extraction:

```toml
[search]
pdf_text = false
```

## SQLite schema sketch

```sql
CREATE TABLE search_items (
  item_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,          -- document, folder, file
  id TEXT,                     -- canonical document/folder ID when applicable
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

CREATE TABLE search_facets (
  item_key TEXT NOT NULL,
  facet TEXT NOT NULL,
  PRIMARY KEY (item_key, facet)
);

CREATE INDEX search_items_kind_idx ON search_items(kind);
CREATE INDEX search_items_type_idx ON search_items(type);
CREATE INDEX search_items_status_idx ON search_items(status);
CREATE INDEX search_facets_facet_idx ON search_facets(facet);

CREATE VIRTUAL TABLE search_fts USING fts5(
  item_key UNINDEXED,
  title,
  path,
  aliases,
  facets,
  metadata,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);
```

`search_fts.item_key` links back to `search_items.item_key`.

## Ranking

Use SQLite FTS5 BM25 ranking with column weights.

Suggested priority:

1. title
2. aliases
3. path
4. facets
5. metadata
6. body

Example conceptual weighting:

```txt
title:    5.0
aliases:  4.0
path:     3.0
facets:   3.0
metadata: 2.0
body:     1.0
```

Ranking should remain deterministic and explainable. Avoid learned ranking in v1.

## Query behavior

Search should support:

- free-text query
- optional kind filter: `document`, `folder`, `file`
- optional type filter
- optional status filter
- optional facet filter
- result limit/offset
- snippets for full-text matches
- facet counts for narrowing results

Empty query may return recent or all indexed items with filters applied, but this can be deferred.

## Server API

Add endpoints:

```txt
GET  /api/search?q=...
GET  /api/search/status
POST /api/search/reindex
```

Optional query params:

```txt
kind=document|folder|file
type=project
status=active
facet=company-x
limit=20
offset=0
```

Response shape:

```ts
type SearchResponse = {
  query: string;
  mode: 'keyword';
  results: SearchResult[];
  facets: SearchFacetCount[];
};

type SearchResult = {
  kind: 'document' | 'folder' | 'file';
  id?: string;
  path: string;
  title?: string;
  type?: string;
  status?: string;
  extension?: string;
  route_token?: string;
  facets: string[];
  snippet?: string;
  score: number;
};

type SearchFacetCount = {
  facet: string;
  count: number;
};
```

## Web UI

Add a simple global search box to the existing read-only UI.

Initial UI behavior:

- Search input in sidebar/header.
- Results panel or route showing ranked results.
- Result cards show title, kind, path, type/status, facets, and snippet.
- Clicking a document opens its existing document route.
- Clicking a folder opens the folder view.
- Clicking a file opens the existing file preview when supported.
- Facet chips can narrow the result set.

No advanced query language is needed for v1.

## Index lifecycle

### Initial build

On server startup:

1. Load `LoadedVault`.
2. Open/create the search database for the vault.
3. Check index status.
4. If missing or stale, either rebuild automatically or report stale status and allow `POST /api/search/reindex`.

For the first implementation, manual rebuild is acceptable.

### Reindex command

`POST /api/search/reindex` should:

1. Clear stale rows or build into a temporary database.
2. Index documents from `LoadedVault`.
3. Walk artifact files, respecting ignore rules.
4. Swap/commit atomically.
5. Return item counts and duration.

### Incremental updates

After the filesystem watcher exists, incremental search updates can use checksums:

- unchanged checksum: skip
- changed file: delete old item/chunks and reindex
- deleted file: delete search rows
- moved file/document: delete old key and insert new key

Until then, full reindex is simpler and acceptable.

## Configuration

Add optional root config later:

```toml
[search]
enabled = true
index_artifacts = true
index_text_artifacts = true
index_pdf_metadata = true
index_pdf_text = false
```

Defaults should keep search lightweight.

## Future v2: semantic/hybrid search

If keyword search is not enough, add optional semantic search:

- SQLite FTS5 remains the keyword/metadata index.
- USearch stores embedding vectors for chunks.
- A local embedding model creates query/document vectors.
- Results from FTS5 and USearch are fused with Reciprocal Rank Fusion.

Hybrid flow:

```txt
query
  -> SQLite FTS5 keyword results
  -> USearch vector results
  -> RRF fusion
  -> final ranked results
```

Use RRF before any learned fusion:

```txt
score(document) = sum(1 / (k + rank_i))
```

Default:

```txt
k = 60
```

Do not add learning-to-rank until there is enough judged query data and end-to-end evaluation.

## Implementation phases

### Phase 1: Core keyword search

- Add `kataan-search` crate or module.
- Add SQLite FTS5 schema and migrations.
- Build index from `LoadedVault` documents and folder indexes.
- Add simple query API in Rust.
- Add tests for document search, alias search, label/facet search, and path search.

### Phase 2: Server API

- Add `/api/search`.
- Add `/api/search/status`.
- Add `/api/search/reindex`.
- Ensure validate/rebuild can mark the search index stale or trigger reindex later.

### Phase 3: Artifact metadata

- Walk non-document files while respecting ignore rules.
- Index artifact filename/path/metadata.
- Add PDF metadata-only indexing.
- Add tests ensuring PDFs do not full-text index body content.

### Phase 4: Web UI

- Add global search input.
- Add result list UI.
- Add result navigation.
- Add facet narrowing.

### Phase 5: Incremental indexing

- Integrate with filesystem watcher once available.
- Update changed/deleted files by checksum.
- Keep full reindex as a fallback.

### Phase 6: Optional hybrid search

- Add embedding/chunking only if needed.
- Add USearch vector sidecar.
- Fuse keyword/vector rankings with RRF.
- Keep semantic search optional and local-only.

## Design decision

For Kataan v1, use **SQLite FTS5**.

Do **not** start with USearch. USearch is a strong future option for vector search, but Kataan first needs reliable local keyword search over Markdown, metadata, paths, labels, and artifact metadata. SQLite gives us that with less complexity and better fit for the current vault model.
