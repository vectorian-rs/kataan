use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use tower::ServiceExt;

use super::render::*;
use super::support::*;
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

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn folder_endpoint_returns_folder_index() {
    let root = test_vault();
    let app = test_app(&root);

    let response = request(app, "GET", "/api/folders/type").await;

    assert_eq!(response.status(), StatusCode::OK);

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

    let response = request(app, "GET", "/api/folder?id=projects%2Fsnappy%2Fsows").await;

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

    for uri in [
        "/api/file?path=projects%2Fleak.json",
        "/api/file/highlight?path=projects%2Fleak.json",
        "/api/file/raw?path=projects%2Fleak.svg",
        "/api/file?path=projects%2Foutside-dir%2Fdata.json",
    ] {
        let response = request(app.clone(), "GET", uri).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
    }

    let response = request(app, "GET", "/api/folder?id=projects").await;
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

    for uri in [
        "/api/file?path=projects%2Fbig.txt",
        "/api/file/highlight?path=projects%2Fbig.txt",
        "/api/file?path=projects%2Fbig.pdf",
        "/api/file/raw?path=projects%2Fbig.pdf",
    ] {
        let response = request(app.clone(), "GET", uri).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
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

async fn json_response<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn request(app: Router, method: &str, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

fn test_app(root: &Path) -> Router {
    router(AppState::new(root.to_path_buf()).unwrap())
}

fn test_vault() -> PathBuf {
    let root = unique_temp_dir();
    kataan_core::init::init_vault(&root, "Test Vault").unwrap();
    root
}

fn unique_temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "kataan-server-test-{}-{counter}",
        std::process::id()
    ))
}
