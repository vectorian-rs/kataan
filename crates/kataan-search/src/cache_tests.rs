//! Compatibility of persisted artifact indexes across the visibility-policy fix.

use super::*;
use std::fs;

fn legacy_cache() -> (tempfile::TempDir, SearchIndex) {
    let root = tempfile::tempdir().unwrap();
    kataan_core::init::init_vault(root.path(), "Legacy cache").unwrap();
    fs::write(
        root.path().join("notes/public.md"),
        "# Public\n\ndocumenttoken\n",
    )
    .unwrap();
    fs::write(
        root.path().join("notes/public.toml"),
        "type = \"note\"\nmarkdown = \"public.md\"\nlabels = [\"documentfacet\"]\n",
    )
    .unwrap();
    fs::write(root.path().join("private.txt"), "privatetoken").unwrap();
    fs::write(root.path().join("visible.txt"), "visibletoken").unwrap();
    let index = SearchIndex::open_default(root.path()).unwrap();
    index
        .reindex_loaded(&LoadedVault::load(root.path()).unwrap())
        .unwrap();
    // The vulnerable index had these exact rows and no policy marker. Do not
    // call the new open/search functions until the simulated upgrade begins.
    let connection = Connection::open(index.path()).unwrap();
    connection
        .execute_batch(
            "DELETE FROM search_metadata WHERE key = 'artifact_ignore_policy';
         INSERT INTO search_facets(item_key, facet)
           SELECT item_key, 'privatefacet' FROM search_items WHERE path = 'private.txt';",
        )
        .unwrap();
    fs::write(root.path().join(".gitignore"), "private.txt\n").unwrap();
    (root, index)
}

fn search(index: &SearchIndex, token: &str) -> SearchResponse {
    index
        .search(&SearchQuery {
            q: Some(token.to_owned()),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn legacy_artifact_cache_is_retired_on_lazy_search_and_upgrade_is_idempotent() {
    let (root, index) = legacy_cache();
    let raw = Connection::open(index.path()).unwrap();
    let old_counts = count_by_kind(&raw).unwrap();
    assert!(old_counts["file"] >= 2);
    // A lazy HTTP handle does not call SearchIndex::open at startup.
    let lazy = SearchIndex::at_default_path(root.path());
    assert!(search(&lazy, "privatetoken").results.is_empty());
    assert_eq!(search(&lazy, "documenttoken").results.len(), 1);
    let listing = lazy.search(&SearchQuery::default()).unwrap();
    assert!(listing
        .facets
        .iter()
        .any(|f| f.facet == "documentfacet" && f.count == 1));
    assert!(!listing.facets.iter().any(|f| f.facet == "privatefacet"));
    let status = lazy.status().unwrap();
    assert_eq!(status.file_count, 0);
    assert_eq!(status.document_count, old_counts["document"]);
    assert_eq!(status.folder_count, old_counts["folder"]);
    assert_eq!(
        status.item_count,
        status.document_count + status.folder_count
    );
    let stale_fts: usize = raw
        .query_row(
            "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'privatetoken'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stale_fts, 0, "retire the content, not just its item row");
    assert_eq!(
        metadata_value(&raw, "artifact_ignore_policy")
            .unwrap()
            .as_deref(),
        Some("gitignore-v1")
    );

    // Normal reindex restores only allowed standalone files. Reopening (the
    // MCP path) or checking status must not retire current-policy files again.
    index
        .reindex_loaded(&LoadedVault::load(root.path()).unwrap())
        .unwrap();
    for _ in 0..2 {
        let reopened = SearchIndex::open_default(root.path()).unwrap();
        assert_eq!(search(&reopened, "visibletoken").results.len(), 1);
        assert!(search(&reopened, "privatetoken").results.is_empty());
        assert!(reopened.status().unwrap().file_count > 0);
    }
}

#[test]
fn legacy_artifact_cache_is_safe_even_if_startup_reindex_fails() {
    let (root, index) = legacy_cache();
    let loaded = LoadedVault::load(root.path()).unwrap();
    // MCP attempts this rebuild at startup, logs a failure, then serves search.
    // Invalidation must commit before that fallible rebuild transaction.
    fs::remove_file(root.path().join("notes/public.md")).unwrap();
    assert!(index.reindex_loaded(&loaded).is_err());
    let fallback = SearchIndex::open_default(root.path()).unwrap();
    assert!(search(&fallback, "privatetoken").results.is_empty());
    assert_eq!(search(&fallback, "documenttoken").results.len(), 1);
    assert_eq!(fallback.status().unwrap().file_count, 0);
}

#[test]
fn legacy_artifact_invalidation_rolls_back_and_fails_closed_on_error() {
    let (root, index) = legacy_cache();
    let raw = Connection::open(index.path()).unwrap();
    raw.execute_batch(
        "CREATE TRIGGER refuse_retirement BEFORE DELETE ON search_items
         WHEN OLD.kind = 'file' BEGIN SELECT RAISE(ABORT, 'retirement refused'); END;",
    )
    .unwrap();
    let result = SearchIndex::at_default_path(root.path()).search(&SearchQuery::default());
    assert!(result.is_err(), "a failed upgrade must not serve old hits");
    assert!(SearchIndex::open_default(root.path()).is_err());
    assert!(metadata_value(&raw, "artifact_ignore_policy")
        .unwrap()
        .is_none());
    let fts: usize = raw
        .query_row(
            "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'privatetoken'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        fts, 1,
        "partial FTS deletion must roll back with the failed items deletion"
    );
    let facets: usize = raw
        .query_row(
            "SELECT COUNT(*) FROM search_facets WHERE facet = 'privatefacet'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(facets, 1);
    raw.execute_batch("DROP TRIGGER refuse_retirement;")
        .unwrap();
    assert!(search(&index, "privatetoken").results.is_empty());
}
