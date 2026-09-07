//! Reading content: folders, documents, files and their previews.

use super::*;

#[tokio::test]
async fn vault_endpoint_returns_root_index() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/vault").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn folders_endpoint_returns_folder_list() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders").await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let folders = body["folders"].as_array().unwrap();
    let projects = folders
        .iter()
        .find(|folder| folder["folder"] == "projects")
        .expect("projects folder present");
    assert_eq!(projects["type"], "project");
    assert!(projects["name"].is_string());
    // The seed vault puts its type-definition documents under `type/`.
    let type_folder = folders
        .iter()
        .find(|folder| folder["folder"] == "type")
        .expect("type folder present");
    assert_eq!(type_folder["document_count"], 7);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn folders_count_includes_nested_documents() {
    let root = test_vault();
    // A document nested two levels under the `projects` type folder.
    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(root.join("projects/alpha/index.md"), "# Alpha\n").unwrap();
    fs::write(
        root.join("projects/alpha/index.toml"),
        "type = \"project\"\nname = \"Alpha\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    fs::write(root.join("projects/alpha/doc1.md"), "# Doc1\n").unwrap();
    fs::write(
        root.join("projects/alpha/doc1.toml"),
        "type = \"project\"\nmarkdown = \"doc1.md\"\n",
    )
    .unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let projects = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["folder"] == "projects")
        .expect("projects folder present");
    // The nested doc1 is attributed to the top-level `projects` folder
    // (folder-index documents like alpha/index are not counted).
    assert_eq!(projects["document_count"], 1);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_folder_is_addressed_by_its_id_as_a_path() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders/type").await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert_eq!(body["id"], "type");
    assert!(body["metadata"].is_object(), "{body}");
    assert_eq!(body["documents"].as_array().unwrap().len(), 7);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn canonical_folder_endpoint_returns_nested_folder() {
    let root = test_vault();
    fs::create_dir_all(root.join("projects/snappy/sows")).unwrap();
    fs::write(root.join("projects/snappy/index.md"), "# Snappy\n").unwrap();
    fs::write(
        root.join("projects/snappy/index.toml"),
        r#"type = "project"
name = "Snappy"
markdown = "index.md"
"#,
    )
    .unwrap();
    fs::write(root.join("projects/snappy/sows/index.md"), "# SOWs\n").unwrap();
    fs::write(
        root.join("projects/snappy/sows/index.toml"),
        r#"type = "project"
name = "SOWs"
markdown = "index.md"
"#,
    )
    .unwrap();
    fs::write(root.join("projects/snappy/sows/demo.md"), "# Demo\n").unwrap();
    fs::write(
        root.join("projects/snappy/sows/demo.toml"),
        r#"type = "project"
markdown = "demo.md"
"#,
    )
    .unwrap();
    let app = test_app(&root);

    let response = request(app.clone(), "GET", "/api/folders/projects/snappy/sows").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    // `sows` holds one document (`demo`) and no subfolders.
    assert!(body["folders"].as_array().unwrap().is_empty());
    let documents = body["documents"].as_array().unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0]["slug"], "demo");

    // The parent lists `sows` as a subfolder child with an index.
    let response = request(app, "GET", "/api/folders/projects/snappy").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let child = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["id"] == "projects/snappy/sows")
        .expect("sows child present");
    assert_eq!(child["has_index"], true);
    assert!(child["name"].is_string());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn folders_list_includes_the_file_backed_code_folder() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let code = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["folder"] == "code")
        .expect("code folder present");
    assert_eq!(code["type"], "code");
    assert_eq!(code["name"], "Code");
    assert_eq!(code["icon"], "Code");
    assert_eq!(code["document_count"], 0);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn one_spelling_addresses_a_resource_however_deep_it_sits() {
    // The reason there were two conventions: `/api/folders/:folder` matched a
    // single path segment, so a nested folder was unreachable by path and a
    // `?id=` spelling existed alongside it — returning a *different type* for
    // the same concept. `*folder` addresses any depth, so one spelling does.
    let root = test_vault();
    fs::create_dir_all(root.join("projects/snappy/sows")).unwrap();
    for folder in ["projects/snappy", "projects/snappy/sows"] {
        fs::write(root.join(folder).join("index.md"), "# Nested\n").unwrap();
        fs::write(
            root.join(folder).join("index.toml"),
            "type = \"project\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
    }
    kataan_core::rebuild::rebuild_indexes(&root).unwrap();

    for (path, id) in [
        ("/api/folders/projects", "projects"),
        ("/api/folders/projects/snappy", "projects/snappy"),
        ("/api/folders/projects/snappy/sows", "projects/snappy/sows"),
    ] {
        let response = request(test_app(&root), "GET", path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let body: serde_json::Value = json_response(response).await;
        assert_eq!(body["id"], id, "{path}");
    }

    // Documents address the same way, and the same way writes always have.
    let response = request(test_app(&root), "GET", "/api/documents/type/note").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert_eq!(body["id"], "type/note");

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_file_backed_folder_answers_rather_than_404s() {
    // `code` is a declared type folder with no folder-index document, so it has
    // no metadata of its own — but it exists and must be listable.
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders/code").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert_eq!(body["id"], "code");
    assert!(body["metadata"].is_null(), "{body}");
    assert!(body["documents"].as_array().unwrap().is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn canonical_code_folder_lists_raw_subdirs_and_files() {
    let root = test_vault();
    fs::create_dir_all(root.join("code/tools")).unwrap();
    fs::write(root.join("code/run.sh"), "#!/bin/sh\n").unwrap();
    fs::write(root.join("code/tools/lib.rs"), "// lib\n").unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders/code").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    // File-backed folders carry no document metadata.
    assert!(body["metadata"].is_null());
    let child = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["id"] == "code/tools")
        .expect("code/tools subdir present");
    // A raw subdir has no folder index.
    assert_eq!(child["has_index"], false);
    assert!(body["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["name"] == "run.sh"));

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn any_indexless_type_folder_is_file_backed_not_404() {
    // Generalization: the file-backed behavior keys off "declared type folder
    // with no folder-index document", not the literal "code".
    let root = test_vault();
    let mut config = fs::read_to_string(root.join("kataan.toml")).unwrap();
    // `[type_folders]` is the last table in the generated config, so appending a
    // mapping lands inside it.
    config.push_str("assets = \"assets\"\n");
    fs::write(root.join("kataan.toml"), config).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders/assets").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert!(body["metadata"].is_null());

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn file_endpoints_reject_symlink_files_and_intermediate_dirs() {
    use std::os::unix::fs::symlink;

    let root = test_vault();
    let outside = unique_temp_dir();
    fs::create_dir_all(outside.join("nested")).unwrap();
    fs::write(outside.join("secret.json"), r#"{"secret":true}"#).unwrap();
    fs::write(outside.join("secret.svg"), "<svg></svg>").unwrap();
    fs::write(outside.join("nested/data.json"), r#"{"nested":true}"#).unwrap();
    symlink(outside.join("secret.json"), root.join("projects/leak.json")).unwrap();
    symlink(outside.join("secret.svg"), root.join("projects/leak.svg")).unwrap();
    symlink(outside.join("nested"), root.join("projects/outside-dir")).unwrap();
    let app = test_app(&root);

    // Symlinked / escaping paths resolve to nothing safe to serve -> 404.
    for uri in [
        "/api/file?path=projects%2Fleak.json",
        "/api/file/highlight?path=projects%2Fleak.json",
        "/api/file/raw?path=projects%2Fleak.svg",
        "/api/file?path=projects%2Foutside-dir%2Fdata.json",
    ] {
        let response = request(app.clone(), "GET", uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }

    let response = request(app, "GET", "/api/folders/projects").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    let files = body["files"].as_array().unwrap();
    assert!(!files.iter().any(|file| file["name"] == "leak.json"));
    assert!(!files.iter().any(|file| file["name"] == "leak.svg"));

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn document_endpoint_returns_document() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/documents/type/project").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn file_endpoint_returns_json_file() {
    let root = test_vault();
    fs::write(root.join("projects/data.json"), r#"{"name":"demo"}"#).unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/file?path=projects%2Fdata.json").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn file_endpoint_returns_html_file() {
    let root = test_vault();
    fs::write(root.join("projects/chart.html"), "<h1>Chart</h1>").unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/file?path=projects%2Fchart.html").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn file_endpoints_return_pdf_file() {
    let root = test_vault();
    fs::write(root.join("projects/report.pdf"), b"%PDF-1.4").unwrap();
    let app = test_app(&root);

    let response = request(app.clone(), "GET", "/api/file?path=projects%2Freport.pdf").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = json_response(response).await;
    assert_eq!(body["kind"], "pdf");
    assert_eq!(body["content"], "");

    let response = request(app, "GET", "/api/file/raw?path=projects%2Freport.pdf").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn file_endpoints_reject_oversized_files() {
    let root = test_vault();
    fs::File::create(root.join("projects/big.txt"))
        .unwrap()
        .set_len(MAX_TEXT_PREVIEW_BYTES + 1)
        .unwrap();
    fs::File::create(root.join("projects/big.pdf"))
        .unwrap()
        .set_len(MAX_RAW_PREVIEW_BYTES + 1)
        .unwrap();
    let app = test_app(&root);

    // Over-limit previews are rejected with 413. The highlight endpoint rejects
    // a `.txt` file earlier, at the "not a highlightable type" check (400),
    // before it ever measures the file.
    for (uri, expected) in [
        (
            "/api/file?path=projects%2Fbig.txt",
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            "/api/file/highlight?path=projects%2Fbig.txt",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/file?path=projects%2Fbig.pdf",
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            "/api/file/raw?path=projects%2Fbig.pdf",
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = request(app.clone(), "GET", uri).await;
        assert_eq!(response.status(), expected, "{uri}");
    }

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn highlight_endpoint_returns_html() {
    let root = test_vault();
    fs::write(root.join("projects/data.json"), r#"{"name":"demo"}"#).unwrap();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/file/highlight?path=projects%2Fdata.json").await;

    assert_eq!(response.status(), StatusCode::OK);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalizes_lumis_line_html_without_extra_blank_lines() {
    let html = "<div class=\"line\">{\n</div><div class=\"line\">}\r\n</div>";

    assert_eq!(
        normalize_lumis_line_html(html),
        "<div class=\"line\">{</div><div class=\"line\">}</div>"
    );
}

#[test]
fn markdown_svg_images_are_rewritten_to_raw_file_api() {
    let html = render_markdown_html(
        "![Look-to-book pollution map](charts/look-to-book.svg)",
        Some("projects/airline-anchor"),
        None,
        &|_| crate::api::render::LinkTarget::Missing,
    )
    .unwrap();

    assert!(
        html.contains("src=\"/api/file/raw?path=projects/airline-anchor/charts/look-to-book.svg\"")
    );
    assert!(html.contains("alt=\"Look-to-book pollution map\""));
}

#[test]
fn markdown_svg_images_do_not_escape_vault() {
    assert_eq!(
        rewrite_markdown_svg_url("../../diagram.svg", Some("projects")),
        None
    );
    assert_eq!(
        rewrite_markdown_svg_url("https://example.com/diagram.svg", Some("projects")),
        None
    );
}
