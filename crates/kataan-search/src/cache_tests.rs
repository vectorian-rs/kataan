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

fn break_cache(index: &SearchIndex, state: &str) {
    let raw = Connection::open(index.path()).unwrap();
    if state == "current-incompatible" {
        // Current-policy caches also need structural recovery. Retire private
        // artifacts before marking this fixture current.
        ensure_artifact_policy(&raw).unwrap();
    }
    if state == "migration-refused" {
        raw.execute_batch(
            "CREATE TRIGGER refuse_retirement BEFORE DELETE ON search_items
             WHEN OLD.kind = 'file' BEGIN SELECT RAISE(ABORT, 'retirement refused'); END;",
        )
        .unwrap();
    } else {
        // Malformed derived schema, not a claim about a released schema version.
        raw.execute_batch(
            "DROP INDEX search_items_status_idx;
             ALTER TABLE search_items DROP COLUMN status;",
        )
        .unwrap();
    }
}

#[test]
fn full_reindex_recovers_incompatible_schema_and_failed_policy_migration() {
    for state in [
        "legacy-incompatible",
        "current-incompatible",
        "migration-refused",
    ] {
        let (root, index) = legacy_cache();
        break_cache(&index, state);
        let lazy = SearchIndex::at_default_path(root.path());
        assert!(lazy.search(&SearchQuery::default()).is_err(), "{state}");
        let response = lazy
            .reindex_loaded(&LoadedVault::load(root.path()).unwrap())
            .unwrap_or_else(|error| panic!("{state}: {error:#}"));
        assert!(response.document_count > 0, "{state}");
        assert!(search(&lazy, "privatetoken").results.is_empty(), "{state}");
        assert_eq!(search(&lazy, "documenttoken").results.len(), 1, "{state}");
        assert_eq!(search(&lazy, "visibletoken").results.len(), 1, "{state}");
        assert_eq!(
            fs::read_to_string(root.path().join("private.txt")).unwrap(),
            "privatetoken"
        );
    }
}

#[test]
fn failed_population_after_cache_recovery_cannot_resurrect_private_rows() {
    for state in [
        "legacy-incompatible",
        "current-incompatible",
        "migration-refused",
    ] {
        let (root, index) = legacy_cache();
        break_cache(&index, state);
        let loaded = LoadedVault::load(root.path()).unwrap();
        fs::remove_file(root.path().join("notes/public.md")).unwrap();
        let error = index.reindex_loaded(&loaded).unwrap_err();
        assert!(
            error.to_string().contains("failed to read markdown"),
            "{state}: {error:#}"
        );
        // Recovery committed before the failed population transaction. Both
        // lazy and eager readers see an empty safe cache, never the old FTS.
        for reader in [
            SearchIndex::at_default_path(root.path()),
            SearchIndex::open_default(root.path()).unwrap(),
        ] {
            assert!(
                search(&reader, "privatetoken").results.is_empty(),
                "{state}"
            );
            let status = reader.status().unwrap();
            assert_eq!(status.item_count, 0, "{state}");
            assert!(status.last_indexed_at.is_none(), "{state}");
        }
        let raw = Connection::open(index.path()).unwrap();
        assert_eq!(
            raw.query_row("SELECT COUNT(*) FROM search_fts", [], |row| row
                .get::<_, usize>(0))
                .unwrap(),
            0,
            "{state}"
        );
        // A subsequent full rebuild works; no manual cache removal needed.
        fs::write(root.path().join("notes/public.md"), "documenttoken").unwrap();
        index.reindex_loaded(&loaded).unwrap();
        assert_eq!(search(&index, "documenttoken").results.len(), 1, "{state}");
        assert!(search(&index, "privatetoken").results.is_empty(), "{state}");
    }
}

#[test]
fn cache_recovery_classifies_extended_preparation_codes_not_messages() {
    use rusqlite::ffi;
    for (code, expected) in [
        (ffi::SQLITE_ERROR, true),
        (ffi::SQLITE_SCHEMA, true),
        (ffi::SQLITE_CONSTRAINT_TRIGGER, true),
        (ffi::SQLITE_BUSY, false),
        (ffi::SQLITE_BUSY_SNAPSHOT, false),
        (ffi::SQLITE_LOCKED, false),
        (ffi::SQLITE_LOCKED_SHAREDCACHE, false),
        (ffi::SQLITE_ERROR_RETRY, false),
        (ffi::SQLITE_ERROR_SNAPSHOT, false),
        (ffi::SQLITE_IOERR, false),
        (ffi::SQLITE_IOERR_WRITE, false),
        (ffi::SQLITE_CANTOPEN, false),
        (ffi::SQLITE_READONLY, false),
        (ffi::SQLITE_FULL, false),
        (ffi::SQLITE_NOMEM, false),
        (ffi::SQLITE_INTERRUPT, false),
        (ffi::SQLITE_PERM, false),
        (ffi::SQLITE_AUTH, false),
        (ffi::SQLITE_CONSTRAINT, false),
        (ffi::SQLITE_CONSTRAINT_NOTNULL, false),
    ] {
        // Both rusqlite forms occur in preparation; anyhow context must not
        // hide the code, nor may an error message grant recovery permission.
        for error in [
            rusqlite::Error::SqliteFailure(ffi::Error::new(code), None),
            rusqlite::Error::SqlInputError {
                error: ffi::Error::new(code),
                msg: "no such column: status".to_owned(),
                sql: "fixture".to_owned(),
                offset: 0,
            },
        ] {
            let error = anyhow::Error::new(error).context("preparing search cache");
            assert_eq!(is_recoverable_preparation_error(&error), expected, "{code}");
        }
    }
    assert!(!is_recoverable_preparation_error(&anyhow::anyhow!(
        "no such column: status"
    )));
}

#[test]
fn contention_then_failed_population_preserves_author_cache_and_schema() {
    use std::cell::{Cell, RefCell};

    // Release the real writer lock at SQLite's failed lock attempt, then tell
    // SQLite not to retry. A catch-all recovery would now be free to commit a
    // destructive reset. No sleep or elapsed-time threshold orders this test.
    thread_local! {
        static LOCK_HOLDER: RefCell<Option<Connection>> = const { RefCell::new(None) };
        static BUSY_CALLS: Cell<usize> = const { Cell::new(0) };
    }
    fn release_on_busy(_: i32) -> bool {
        BUSY_CALLS.with(|calls| calls.set(calls.get() + 1));
        LOCK_HOLDER.with(|holder| {
            holder
                .borrow_mut()
                .take()
                .unwrap()
                .execute_batch("ROLLBACK")
                .unwrap();
        });
        false
    }

    let (root, index) = legacy_cache();
    let loaded = LoadedVault::load(root.path()).unwrap();
    fs::remove_file(root.path().join("notes/public.md")).unwrap();
    let mut connection = index.open_database().unwrap();
    let schema_version = |connection: &Connection| {
        connection
            .pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))
            .unwrap()
    };
    let before_schema = schema_version(&connection);
    let before_counts = count_by_kind(&connection).unwrap();
    let before_indexed_at = read_last_indexed_at(&connection).unwrap();
    assert!(before_counts["document"] > 0);
    let blocker = Connection::open(index.path()).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    LOCK_HOLDER.with(|holder| *holder.borrow_mut() = Some(blocker));
    connection.busy_handler(Some(release_on_busy)).unwrap();

    // This is the production full-reindex preparation, on a normally configured
    // connection with only its busy-timeout handler replaced for synchronization.
    let preparation = prepare_reindex(&mut connection);
    assert_eq!(
        BUSY_CALLS.with(Cell::get),
        1,
        "prove actual SQLite contention"
    );
    assert!(LOCK_HOLDER.with(|holder| holder.borrow().is_none()));
    assert_eq!(
        schema_version(&connection),
        before_schema,
        "contention must not reset a healthy schema"
    );
    assert_eq!(count_by_kind(&connection).unwrap(), before_counts);
    let error = preparation.unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<rusqlite::Error>()
            .unwrap()
            .sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    assert!(metadata_value(&connection, "artifact_ignore_policy")
        .unwrap()
        .is_none());

    // After the transient failure, a genuine retry retires private files, then
    // fails reading Markdown. Its rollback must retain the author documents.
    let error = index.reindex_loaded(&loaded).unwrap_err();
    assert!(
        error.to_string().contains("failed to read markdown"),
        "{error:#}"
    );
    assert_eq!(schema_version(&connection), before_schema);
    assert_eq!(
        read_last_indexed_at(&connection).unwrap(),
        before_indexed_at
    );
    let after_counts = count_by_kind(&connection).unwrap();
    assert_eq!(after_counts["document"], before_counts["document"]);
    assert_eq!(after_counts["folder"], before_counts["folder"]);
    assert_eq!(after_counts.get("file"), None);
    assert!(search(&index, "privatetoken").results.is_empty());
    assert_eq!(search(&index, "documenttoken").results.len(), 1);
}
