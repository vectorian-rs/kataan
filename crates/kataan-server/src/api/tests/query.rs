//! Addressing and querying: canonical ids as routes, the graph, the schema,
//! and what the vault says about itself.

use super::*;

#[tokio::test]
async fn watch_endpoint_returns_status() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/watch").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn query_document_endpoint_returns_nested_document() {
    let root = test_vault();
    fs::create_dir_all(root.join("projects/snappy/sows/otp-travel")).unwrap();
    fs::write(
        root.join("projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429.md"),
        "# Demo\n",
    )
    .unwrap();
    fs::write(
        root.join("projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429.toml"),
        r#"type = "project"
markdown = "HU-otp-travel-POC-SOW1-260429.md"
"#,
    )
    .unwrap();
    let app = test_app(&root);

    let response = request(
        app,
        "GET",
        "/api/document?id=projects%2Fsnappy%2Fsows%2Fotp-travel%2FHU-otp-travel-POC-SOW1-260429",
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

/// The HTTP surface must answer the same discovery questions as MCP: what does
/// this type require, and what may connect to what.
#[tokio::test]
async fn schema_and_ontology_serve_the_vaults_own_model() {
    let root = test_vault();
    let path = root.join("ontology.toml");
    let existing = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!(
            "{existing}\n[nodes.person]\nrequired = [\"email\"]\n\n\
             [nodes.person.fields]\nemail = {{ type = \"string\" }}\n"
        ),
    )
    .unwrap();

    let response = request(test_app(&root), "GET", "/api/schema/person").await;
    assert_eq!(response.status(), StatusCode::OK);
    let schema: serde_json::Value = json_response(response).await;
    assert_eq!(schema["node_schema"]["required"][0], "email");

    let response = request(test_app(&root), "GET", "/api/ontology").await;
    assert_eq!(response.status(), StatusCode::OK);
    let ontology: serde_json::Value = json_response(response).await;
    assert!(ontology["types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|ty| ty["name"] == "person"));
    assert!(!ontology["links"].as_array().unwrap().is_empty());

    // A kind that is neither a kataan schema nor a vault type is still a 404.
    let response = request(test_app(&root), "GET", "/api/schema/nonsense").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn an_oversized_subgraph_is_a_bad_request_not_a_huge_response() {
    let root = test_vault();
    for slug in ["alpha", "beta"] {
        fs::write(root.join(format!("notes/{slug}.md")), format!("# {slug}\n")).unwrap();
        fs::write(
            root.join(format!("notes/{slug}.toml")),
            format!("type = \"note\"\nmarkdown = \"{slug}.md\"\n"),
        )
        .unwrap();
    }
    kataan_core::rebuild::rebuild_indexes(&root).unwrap();

    // Unfiltered is fine: the ceiling permits a whole-vault export, which is
    // what the graph view and `kataan graph export` both want.
    let response = request(test_app(&root), "GET", "/api/graph/subgraph").await;
    assert_eq!(response.status(), StatusCode::OK);
    let full: serde_json::Value = json_response(response).await;
    let total = full["nodes"].as_array().unwrap().len();
    assert!(total > 1, "fixture too small to exceed a limit");

    // A caller-lowered ceiling is refused as a request error rather than
    // answered with a partial graph.
    let response = request(
        test_app(&root),
        "GET",
        &format!("/api/graph/subgraph?limit={}", total - 1),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Above the hard maximum is refused too, so `limit` cannot be used to opt
    // out of the ceiling.
    let response = request(
        test_app(&root),
        "GET",
        &format!(
            "/api/graph/subgraph?limit={}",
            kataan_core::query::MAX_SUBGRAPH_NODES + 1
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn internal_document_links_become_app_routes_end_to_end() {
    let root = test_vault();

    // Two real documents, one linking to the other the way a vault actually
    // does: a bare sibling filename.
    for (slug, body) in [
        (
            "alpha",
            "# Alpha\n\nSee [Beta](beta.md) and [gone](deleted.md).\n",
        ),
        ("beta", "# Beta\n"),
    ] {
        fs::write(root.join(format!("notes/{slug}.md")), body).unwrap();
        fs::write(
            root.join(format!("notes/{slug}.toml")),
            format!("type = \"note\"\nmarkdown = \"{slug}.md\"\n"),
        )
        .unwrap();
    }
    kataan_core::rebuild::rebuild_indexes(&root).unwrap();

    let app = test_app(&root);
    let response = request(app, "GET", "/api/document?id=notes/alpha").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let html = body["html"].as_str().unwrap();

    // The live link points at the app's own route and is marked for in-app
    // selection, so following it does not reload the page.
    assert!(
        html.contains(r#"href="/notes/beta""#),
        "sibling link not rewritten: {html}"
    );
    assert!(
        html.contains(r#"data-document="notes/beta""#),
        "missing selection marker: {html}"
    );
    // The dead one is left exactly as authored rather than silently
    // resolving to something that resets the app.
    assert!(
        html.contains(r#"href="deleted.md""#),
        "dead link was rewritten: {html}"
    );

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_document_route_is_its_canonical_id() {
    let root = test_vault();
    let app = test_app(&root);

    // The id is the route: no token, no lookup table.
    let response = request(app, "GET", "/api/resolve-path?path=type/project.md").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert_eq!(body["id"], "type/project");
    assert!(
        body.get("route_token").is_none(),
        "route_token should be gone: {body}"
    );
    // The whole projection, not just the id: this is the shape MCP returns too,
    // and the two drifted apart once already.
    assert_eq!(body["folder"], "type");
    assert_eq!(body["type_folder"], "type");
    assert_eq!(body["is_folder_index"], false);

    fs::remove_dir_all(root).unwrap();
}
