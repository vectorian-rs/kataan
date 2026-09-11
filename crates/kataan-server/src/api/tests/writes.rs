//! Routes that change the vault, and the ones that rebuild what is derived
//! from it.

use super::*;

#[tokio::test]
async fn validate_endpoint_returns_diagnostics() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "POST", "/api/validate").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn rebuild_indexes_endpoint_repairs_indexes() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "POST", "/api/rebuild-indexes").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn search_endpoints_reindex_and_query_documents() {
    let root = test_vault();
    let default_index_path = kataan_search::default_index_path(&root);
    let _ = fs::remove_file(&default_index_path);
    fs::write(
        root.join("notes/session-store.md"),
        "# Session Store\n\nThe durable platypus store coordinates local search state.",
    )
    .unwrap();
    fs::write(
        root.join("notes/session-store.toml"),
        r#"type = "note"
status = "active"
markdown = "session-store.md"
aliases = ["session cache"]
labels = ["local-first"]
"#,
    )
    .unwrap();

    let app = test_app(&root);

    let status_response = request(app.clone(), "GET", "/api/search/status").await;
    assert_eq!(status_response.status(), StatusCode::OK);
    let status: kataan_search::SearchStatus = json_response(status_response).await;
    assert!(!status.exists);

    let reindex_response = request(app.clone(), "POST", "/api/search/reindex").await;
    assert_eq!(reindex_response.status(), StatusCode::OK);
    let reindex: kataan_search::ReindexResponse = json_response(reindex_response).await;
    assert!(reindex.document_count > 0);

    let search_response = request(
        app.clone(),
        "GET",
        "/api/search?q=platypus&kind=document&type=note&status=active&facet=local-first",
    )
    .await;
    assert_eq!(search_response.status(), StatusCode::OK);
    let search: kataan_search::SearchResponse = json_response(search_response).await;
    assert!(search
        .results
        .iter()
        .any(|result| result.id.as_deref() == Some("notes/session-store")));

    let alias_response = request(app, "GET", "/api/search?q=session%20cache").await;
    assert_eq!(alias_response.status(), StatusCode::OK);
    let alias_search: kataan_search::SearchResponse = json_response(alias_response).await;
    assert!(alias_search
        .results
        .iter()
        .any(|result| result.id.as_deref() == Some("notes/session-store")));

    let _ = fs::remove_file(default_index_path);
    fs::remove_dir_all(root).unwrap();
}

/// Writes existed only over MCP, so a browser UI could read the vault and not
/// change it, and no script or CI job could write at all.
#[tokio::test]
async fn the_http_api_can_create_update_and_link_documents() {
    let root = test_vault();

    // Create.
    let response = json_request(
        test_app(&root),
        "POST",
        "/api/documents",
        serde_json::json!({ "type": "note", "title": "Written Over HTTP", "body": "hello" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: serde_json::Value = json_response(response).await;
    assert_eq!(created["id"], "notes/written-over-http");

    // The write is visible to reads immediately, not after the next reload.
    let response = request(
        test_app(&root),
        "GET",
        "/api/documents/notes/written-over-http",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Update.
    let response = json_request(
        test_app(&root),
        "PATCH",
        "/api/documents/notes/written-over-http",
        serde_json::json!({ "body": "goodbye", "status": "active" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = request(
        test_app(&root),
        "GET",
        "/api/documents/notes/written-over-http",
    )
    .await;
    let document: serde_json::Value = json_response(response).await;
    assert_eq!(document["markdown"], "goodbye");
    assert_eq!(document["metadata"]["status"], "active");

    // Edges: add, replace, remove — the whole surface MCP has.
    let response = json_request(
        test_app(&root),
        "POST",
        "/api/documents",
        serde_json::json!({ "type": "topic", "title": "Linked", "body": "x" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let edge = serde_json::json!({
        "source": "notes/written-over-http",
        "predicate": "related_to",
        "target": "topics/linked"
    });
    assert_eq!(
        json_request(test_app(&root), "POST", "/api/edges", edge.clone())
            .await
            .status(),
        StatusCode::OK
    );

    let response = request(
        test_app(&root),
        "GET",
        "/api/graph/neighbors?id=notes/written-over-http",
    )
    .await;
    let neighbors: serde_json::Value = json_response(response).await;
    assert_eq!(neighbors["out"]["related_to"][0]["id"], "topics/linked");

    assert_eq!(
        json_request(
            test_app(&root),
            "PUT",
            "/api/edges",
            serde_json::json!({
                "source": "notes/written-over-http",
                "predicate": "related_to",
                "targets": ["topics/linked"]
            })
        )
        .await
        .status(),
        StatusCode::OK
    );

    let response = request(
        test_app(&root),
        "DELETE",
        "/api/edges?source=notes/written-over-http&predicate=related_to&target=topics/linked",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = request(
        test_app(&root),
        "GET",
        "/api/graph/neighbors?id=notes/written-over-http",
    )
    .await;
    let neighbors: serde_json::Value = json_response(response).await;
    assert!(neighbors["out"]["related_to"].is_null());

    fs::remove_dir_all(root).unwrap();
}

/// A write rejected by the vault's rules is the caller's mistake. It must not
/// surface as a 500, or a client cannot tell "you asked for something illegal"
/// from "the server broke".
#[tokio::test]
async fn an_illegal_write_is_a_client_error_not_a_server_error() {
    let root = test_vault();

    for body in [
        serde_json::json!({ "type": "no-such-type", "title": "X", "body": "y" }),
        serde_json::json!({ "type": "note", "title": "X", "body": "y", "occurred_at": "2026" }),
    ] {
        let response = json_request(test_app(&root), "POST", "/api/documents", body).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "expected 400, got {}",
            response.status()
        );
    }

    fs::remove_dir_all(root).unwrap();
}

/// The write routes take no body a form could not send, so without this a page
/// on any site the user is visiting could create documents in their vault.
#[tokio::test]
async fn write_routes_refuse_a_cross_site_request() {
    let root = test_vault();

    let response = cross_site_request(
        test_app(&root),
        "POST",
        "/api/documents",
        serde_json::json!({ "type": "note", "title": "Forged", "body": "x" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // And nothing was written.
    let response = request(test_app(&root), "GET", "/api/documents/notes/forged").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    fs::remove_dir_all(root).unwrap();
}

// Holding a `MutexGuard` across an await is normally a deadlock waiting to
// happen, which is why clippy refuses it — but holding it across one is the
// entire experiment here: the assertion is that maintenance *cannot* proceed
// while the lock is held. The handler runs on the blocking pool, so nothing
// this task awaits depends on the guard being released.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn maintenance_waits_for_the_writer_lock() {
    // `rebuild-indexes` rewrites every folder index and sidecar checksum, so it
    // is a writer: while only mutations took the lock, a rebuild could read a
    // sidecar, a PATCH could be acknowledged, and the rebuild could then
    // persist its older reconstruction over the top.
    //
    // Racing two real requests does not test this — the window between that
    // read and its write is microseconds, and such a test passes whether or not
    // the lock is taken, which makes it worse than no test. So the property
    // itself is asserted: with the writer lock held, maintenance waits.
    let root = test_vault();
    let state = AppState::new(root.clone()).unwrap();

    // Taken before the request is spawned, so the request cannot slip past.
    let guard = state.lock_writes();
    let app = router(state.clone());
    let mut pending = tokio::spawn(async move {
        json_request(app, "POST", "/api/rebuild-indexes", serde_json::json!({})).await
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(400), &mut pending)
            .await
            .is_err(),
        "rebuild-indexes ran while the vault writer lock was held"
    );

    drop(guard);
    let response = pending.await.expect("rebuild task");
    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(&root).unwrap();
}
