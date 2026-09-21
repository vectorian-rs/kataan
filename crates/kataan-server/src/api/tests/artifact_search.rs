use super::*;

#[tokio::test]
async fn legacy_artifact_cache_cannot_leak_on_normal_http_search() {
    let root = test_vault();
    fs::write(root.join("private.txt"), "privatetoken").unwrap();
    fs::write(root.join("visible.txt"), "visibletoken").unwrap();
    fs::write(root.join("notes/public.md"), "# Public\n\ndocumenttoken\n").unwrap();
    fs::write(
        root.join("notes/public.toml"),
        "type = \"note\"\nmarkdown = \"public.md\"\n",
    )
    .unwrap();
    let index = kataan_search::SearchIndex::open_default(&root).unwrap();
    index
        .reindex_loaded(&kataan_core::vault::LoadedVault::load(&root).unwrap())
        .unwrap();
    // Simulate the old writer's persisted rows and missing policy marker.
    let raw = rusqlite::Connection::open(index.path()).unwrap();
    raw.execute(
        "DELETE FROM search_metadata WHERE key = 'artifact_ignore_policy'",
        [],
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "private.txt\n").unwrap();
    let app = test_app(&root);
    // No manual reindex and no explicit index-open in AppState's lazy handle.
    let response = request(app.clone(), "GET", "/api/search?q=privatetoken").await;
    assert_eq!(response.status(), StatusCode::OK);
    let search: kataan_search::SearchResponse = json_response(response).await;
    assert!(
        search.results.is_empty(),
        "legacy private snippet leaked: {:?}",
        search.results
    );
    let response = request(app.clone(), "GET", "/api/search?q=documenttoken").await;
    let search: kataan_search::SearchResponse = json_response(response).await;
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].id.as_deref(), Some("notes/public"));
    let response = request(app.clone(), "GET", "/api/search/status").await;
    let status: kataan_search::SearchStatus = json_response(response).await;
    assert_eq!(
        status.file_count, 0,
        "one-time artifact reset, not document removal"
    );
    assert!(status.document_count > 0);

    assert_eq!(
        request(app.clone(), "POST", "/api/search/reindex")
            .await
            .status(),
        StatusCode::OK
    );
    for (token, expected) in [("privatetoken", 0), ("visibletoken", 1)] {
        let response = request(app.clone(), "GET", &format!("/api/search?q={token}")).await;
        let search: kataan_search::SearchResponse = json_response(response).await;
        assert_eq!(search.results.len(), expected, "{token}");
    }
    fs::remove_dir_all(root).unwrap();
}

/// The standalone-file index must not reveal text the file route hides. Scan
/// rules still apply independently; a gitignore negation cannot override them.
#[tokio::test]
async fn standalone_search_respects_serving_and_scan_exclusions() {
    let root = test_vault();
    fs::write(
        root.join(".gitignore"),
        "private/\n*.txt\n!visible.txt\n!scan-hidden.txt\n!kataan-hidden.txt\n!target/keep.txt\nnotes/author.*\n",
    )
    .unwrap();
    fs::write(root.join(".kataanignore"), "kataan-hidden.txt\n").unwrap();
    let config_path = root.join("kataan.toml");
    let mut config = fs::read_to_string(&config_path)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    config.as_table_mut().unwrap().insert(
        "scan".to_owned(),
        toml::toml! { ignore = ["scan-hidden.txt"] }.into(),
    );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();

    for (path, token) in [
        ("private/data.txt", "privatetoken"),
        ("hidden.txt", "hiddentoken"),
        ("visible.txt", "visibletoken"),
        ("scan-hidden.txt", "scantoken"),
        ("kataan-hidden.txt", "kataantoken"),
        ("target/keep.txt", "buildtoken"),
        (".hidden/data.json", "dottoken"),
        ("package-lock.json", "locktoken"),
    ] {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, token).unwrap();
    }
    // Do not make root .gitignore change author-document scanning.
    fs::write(root.join("notes/author.md"), "# Authortoken\n").unwrap();
    fs::write(
        root.join("notes/author.toml"),
        "type = \"note\"\nmarkdown = \"author.md\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    let outside = {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.json"), "outsidetoken").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("outside.json"), root.join("link.json"))
            .unwrap();
        outside
    };

    let app = test_app(&root);
    assert_eq!(
        request(app.clone(), "POST", "/api/search/reindex")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(app.clone(), "GET", "/api/file?path=private/data.txt")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(app.clone(), "GET", "/api/file?path=visible.txt")
            .await
            .status(),
        StatusCode::OK
    );
    for token in [
        "privatetoken",
        "hiddentoken",
        "scantoken",
        "kataantoken",
        "buildtoken",
        "dottoken",
        "locktoken",
        "outsidetoken",
    ] {
        let response = request(app.clone(), "GET", &format!("/api/search?q={token}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let search: kataan_search::SearchResponse = json_response(response).await;
        assert!(
            search.results.is_empty(),
            "{token} leaked: {:?}",
            search.results
        );
    }
    for (token, path) in [
        ("visibletoken", "visible.txt"),
        ("authortoken", "notes/author.md"),
    ] {
        let response = request(app.clone(), "GET", &format!("/api/search?q={token}")).await;
        let search: kataan_search::SearchResponse = json_response(response).await;
        assert_eq!(search.results.len(), 1, "visible control {token}");
        assert_eq!(search.results[0].path, path);
    }
    #[cfg(unix)]
    drop(outside);
    fs::remove_dir_all(root).unwrap();
}
