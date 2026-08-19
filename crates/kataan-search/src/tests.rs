use std::fs;

use super::*;

#[test]
fn connections_use_wal_for_concurrent_read_and_reindex() {
    let root = temp_dir("wal");
    kataan_core::init::init_vault(&root, "Search Test").unwrap();
    let index = SearchIndex::open(root.join("search.sqlite")).unwrap();

    let connection = index.connect().unwrap();
    let mode: String = connection
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");

    fs::remove_dir_all(root).unwrap();
}

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
    fs::create_dir_all(root.join("projects/company-x-extra")).unwrap();
    fs::write(
        root.join("projects/company-x-extra/q2-launch.md"),
        "# Sibling Launch\n\nLaunch notes in a sibling prefix.",
    )
    .unwrap();
    fs::write(
        root.join("projects/company-x-extra/q2-launch.toml"),
        r#"type = "project"
status = "active"
markdown = "q2-launch.md"
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
    let prefix_response = index
        .search(&SearchQuery {
            q: Some("launch".to_owned()),
            path_prefix: Some("projects/company-x".to_owned()),
            ..SearchQuery::default()
        })
        .unwrap();
    assert!(prefix_response
        .results
        .iter()
        .any(|result| result.id.as_deref() == Some("projects/company-x/q2-launch")));
    assert!(!prefix_response
        .results
        .iter()
        .any(|result| result.id.as_deref() == Some("projects/company-x-extra/q2-launch")));

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

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reindex_rebuilds_a_stale_schema_index() {
    let root = temp_dir("stale-schema");
    kataan_core::init::init_vault(&root, "Search Test").unwrap();
    write_note(
        &root,
        "note-a",
        "# Note A\n",
        "type = \"note\"\nstatus = \"active\"\nmarkdown = \"note-a.md\"\n",
    );
    let db_path = root.join("search.sqlite");

    // Simulate an index built by an older extractor: search_items with the
    // removed NOT NULL columns (checksum/extractor_version/indexed_at).
    {
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE search_items (
                   item_key TEXT PRIMARY KEY, kind TEXT NOT NULL, id TEXT, path TEXT NOT NULL,
                   title TEXT, type TEXT, status TEXT, extension TEXT, route_token TEXT,
                   checksum TEXT NOT NULL, extractor_version TEXT NOT NULL, indexed_at TEXT NOT NULL
                 );",
            )
            .unwrap();
    }

    let loaded = LoadedVault::load(&root).unwrap();
    let index = SearchIndex::open(&db_path).unwrap();
    // Reindex must succeed by rebuilding the schema, not fail inserting into the
    // stale table's dropped NOT NULL columns.
    let response = index.reindex_loaded(&loaded).unwrap();
    assert!(response.document_count > 0);
    assert!(index.status().unwrap().item_count > 0);

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
