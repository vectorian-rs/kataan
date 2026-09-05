use std::fs;

use super::*;

/// An initialised vault with one indexed note, plus its index.
fn indexed_vault(name: &str) -> (std::path::PathBuf, SearchIndex) {
    let root = temp_dir(name);
    kataan_core::init::init_vault(&root, "Search Test").unwrap();
    write_note(
        &root,
        "sample",
        "# Sample\n\nSearchable body text.",
        "type = \"note\"\nmarkdown = \"sample.md\"\n",
    );
    let loaded = LoadedVault::load(&root).unwrap();
    let index = SearchIndex::open(root.join("search.sqlite")).unwrap();
    index.reindex_loaded(&loaded).unwrap();
    (root, index)
}

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

#[test]
fn a_query_with_no_searchable_terms_matches_nothing() {
    let (root, index) = indexed_vault("no-terms");

    // Every one of these has query text but no alphanumeric token. Falling
    // through to the unfiltered listing presented the whole vault as keyword
    // matches, so a user searching `C++` saw everything.
    for q in ["@#$%", "(((", "&&", "\u{1f600}", "+++"] {
        let response = index
            .search(&SearchQuery {
                q: Some(q.to_owned()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            response.results.is_empty(),
            "`{q}` returned {} results",
            response.results.len()
        );
    }

    // No query text at all is still a listing, not a no-op.
    let listing = index.search(&SearchQuery::default()).unwrap();
    assert!(!listing.results.is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_corrupt_index_is_rebuilt_rather_than_failing_forever() {
    let (dir, index) = indexed_vault("corrupt-index");
    let path = index.path().to_path_buf();
    assert!(path.exists());

    // Truncate the cache to garbage, as an interrupted write would.
    std::fs::write(&path, b"this is not a database").unwrap();

    // Opening recreates it instead of returning "file is not a database"
    // forever at a cache path the user cannot find.
    let reopened = SearchIndex::open(&path).unwrap();
    let response = reopened.search(&SearchQuery::default()).unwrap();
    assert!(response.results.is_empty(), "a rebuilt index starts empty");

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_absurd_offset_returns_nothing_rather_than_page_one() {
    let (root, index) = indexed_vault("offset-clamp");

    let response = index
        .search(&SearchQuery {
            offset: Some(usize::MAX),
            ..Default::default()
        })
        .unwrap();

    assert!(
        response.results.is_empty(),
        "an overflowing offset wrapped negative and re-read the first page"
    );

    fs::remove_dir_all(root).unwrap();
}

/// A write touches one document, so the index should too — and the row it
/// replaces has to go, or the old body keeps matching.
#[test]
fn refresh_document_replaces_one_entry_without_rebuilding() {
    let (root, index) = indexed_vault("refresh-one");
    let before = index.status().unwrap().item_count;

    let id = kataan_core::mutate::create_document(
        &root,
        kataan_core::mutate::NewDocument {
            r#type: "note".to_owned(),
            title: "Distinctive".to_owned(),
            body: "florbulate".to_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let loaded = LoadedVault::load(&root).unwrap();
    assert!(index.refresh_document(&loaded, &id).unwrap());
    assert_eq!(index.status().unwrap().item_count, before + 1);
    assert!(hits(&index, "florbulate").contains(&id.as_str().to_owned()));

    // Rewriting the body must retire the old text, not shadow it.
    kataan_core::mutate::update_document(
        &root,
        &id,
        Some("quuxify".to_owned()),
        Default::default(),
    )
    .unwrap();
    let loaded = LoadedVault::load(&root).unwrap();
    assert!(index.refresh_document(&loaded, &id).unwrap());

    assert!(hits(&index, "quuxify").contains(&id.as_str().to_owned()));
    assert!(
        hits(&index, "florbulate").is_empty(),
        "the replaced body still matches; the old row was not deleted"
    );
    // And exactly one entry for it, not two.
    assert_eq!(index.status().unwrap().item_count, before + 1);

    fs::remove_dir_all(root).unwrap();
}

fn hits(index: &SearchIndex, query: &str) -> Vec<String> {
    index
        .search(&SearchQuery {
            q: Some(query.to_owned()),
            ..Default::default()
        })
        .unwrap()
        .results
        .into_iter()
        .filter_map(|result| result.id)
        .collect()
}

/// Facet counts describe the match set, not the page.
///
/// Counting the returned rows made every number a function of `limit` and hid
/// any facet with no hit on the current page — so a sidebar built on them both
/// under-reported and omitted the options it exists to offer.
#[test]
fn facet_counts_cover_the_whole_match_set_not_the_page() {
    let root = temp_dir("facet-counts");
    kataan_core::init::init_vault(&root, "Facets").unwrap();
    for index in 0..12 {
        write_note(
            &root,
            &format!("note-{index}"),
            "# Note\n\nshared searchable body",
            "type = \"note\"\nmarkdown = \"note-INDEX.md\"\nlabels = [\"shared\"]\n"
                .replace("INDEX", &index.to_string())
                .as_str(),
        );
    }
    let loaded = LoadedVault::load(&root).unwrap();
    let index = SearchIndex::open(root.join("search.sqlite")).unwrap();
    index.reindex_loaded(&loaded).unwrap();

    let count_of = |limit: usize| {
        index
            .search(&SearchQuery {
                q: Some("searchable".to_owned()),
                limit: Some(limit),
                ..Default::default()
            })
            .unwrap()
    };

    let small = count_of(3);
    let large = count_of(100);

    assert_eq!(small.results.len(), 3, "the page is still bounded by limit");
    assert!(large.results.len() > 3);
    assert_eq!(
        small.facets, large.facets,
        "facet counts changed with the page size"
    );

    let shared = small
        .facets
        .iter()
        .find(|facet| facet.facet == "shared")
        .expect("the shared label is a facet");
    assert_eq!(
        shared.count, 12,
        "the count should be every match, not the three on this page"
    );

    fs::remove_dir_all(root).unwrap();
}
