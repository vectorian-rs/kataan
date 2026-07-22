use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use kataan_core::title::title_from_id;

use crate::{state::AppState, watch::WatchStatus};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub loaded: bool,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct FoldersResponse {
    pub folders: Vec<FolderSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub struct FolderSummaryResponse {
    pub r#type: String,
    pub folder: String,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub document_count: usize,
}

#[derive(Debug, Serialize)]
pub struct FolderResponse {
    pub folder: String,
    pub index: kataan_core::index::FolderIndex,
    pub documents: Vec<FolderDocumentResponse>,
}

#[derive(Debug, Serialize)]
pub struct CanonicalFolderResponse {
    pub id: String,
    pub metadata: Option<kataan_core::document::DocumentMetadata>,
    pub markdown: Option<String>,
    pub folders: Vec<FolderChildResponse>,
    pub documents: Vec<FolderDocumentResponse>,
    pub files: Vec<FolderFileResponse>,
}

#[derive(Debug, Serialize)]
pub struct FolderChildResponse {
    pub id: String,
    pub name: String,
    pub has_index: bool,
}

#[derive(Debug, Serialize)]
pub struct FolderDocumentResponse {
    pub id: String,
    pub slug: String,
    pub markdown: String,
    pub toml: String,
}

#[derive(Debug, Serialize)]
pub struct FolderFileResponse {
    pub name: String,
    pub path: String,
    pub extension: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocumentResponse {
    pub id: String,
    pub type_folder: String,
    pub route_token: String,
    pub metadata: kataan_core::document::DocumentMetadata,
    pub markdown: String,
    pub html: String,
}

#[derive(Debug, Serialize)]
pub struct FileResponse {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct HighlightResponse {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub language: String,
    pub html: String,
}

#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub id: String,
    pub folder: String,
    pub type_folder: String,
    pub route_token: String,
    pub is_folder_index: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticResponse>,
}

#[derive(Debug, Deserialize)]
pub struct IdQuery {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: String,
    pub theme: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub r#type: String,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticResponse {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/watch", get(watch_status))
        .route("/api/search", get(search))
        .route("/api/search/status", get(search_status))
        .route("/api/search/reindex", post(search_reindex))
        .route("/api/vault", get(vault))
        .route("/api/folders", get(folders))
        .route("/api/folder", get(folder_by_id))
        .route("/api/document", get(document_by_id))
        .route("/api/file", get(file_by_path))
        .route("/api/file/raw", get(raw_file_by_path))
        .route("/api/file/highlight", get(highlight_file_by_path))
        .route("/api/resolve", get(resolve_route))
        .route("/api/schema/:kind", get(schema))
        .route("/api/folders/:folder", get(folder))
        .route("/api/documents/*id", get(document))
        .route("/api/validate", post(validate))
        .route("/api/rebuild-indexes", post(rebuild_indexes))
        .with_state(state)
}

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let loaded = state.vault.read().is_ok();
    Json(HealthResponse { ok: true, loaded })
}

pub async fn watch_status(State(state): State<AppState>) -> Json<WatchStatus> {
    let status = state
        .watch
        .read()
        .map(|status| status.clone())
        .unwrap_or_default();
    Json(status)
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<kataan_search::SearchQuery>,
) -> Result<Json<kataan_search::SearchResponse>, ApiError> {
    let index = kataan_search::SearchIndex::open_default(state.vault_path.as_ref())
        .map_err(ApiError::from)?;
    index.search(&query).map(Json).map_err(ApiError::from)
}

pub async fn search_status(
    State(state): State<AppState>,
) -> Result<Json<kataan_search::SearchStatus>, ApiError> {
    kataan_search::SearchIndex::status_for_vault(state.vault_path.as_ref())
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn search_reindex(
    State(state): State<AppState>,
) -> Result<Json<kataan_search::ReindexResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let index = kataan_search::SearchIndex::open_default(state.vault_path.as_ref())
        .map_err(ApiError::from)?;
    index
        .reindex_loaded(&loaded)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn vault(
    State(state): State<AppState>,
) -> Result<Json<kataan_core::index::VaultConfig>, ApiError> {
    if let Ok(loaded) = read_loaded_vault(&state) {
        return Ok(Json(loaded.index.clone()));
    }

    let vault =
        kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)?;
    Ok(Json(vault.index))
}

pub async fn folders(State(state): State<AppState>) -> Result<Json<FoldersResponse>, ApiError> {
    let mut folders = Vec::new();

    if let Ok(loaded) = read_loaded_vault(&state) {
        for (ty, folder) in &loaded.index.type_folders {
            let id = kataan_core::id::CanonicalId::parse(folder)
                .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
            let record = loaded.documents.get(&id);
            let document_count = recursive_document_count(&loaded, folder);
            folders.push(FolderSummaryResponse {
                r#type: ty.clone(),
                folder: folder.clone(),
                name: Some(
                    record
                        .and_then(document_name)
                        .unwrap_or_else(|| title_from_id(folder)),
                ),
                icon: loaded
                    .type_registry
                    .definitions
                    .get(ty)
                    .and_then(|definition| definition.icon.clone()),
                document_count,
            });
        }
        push_code_folder_if_needed(
            &state,
            loaded
                .index
                .type_folders
                .values()
                .any(|folder| kataan_core::constants::is_code_folder(folder)),
            &mut folders,
        );
        return Ok(Json(FoldersResponse { folders }));
    }

    let vault =
        kataan_core::vault::Vault::open(state.vault_path.as_ref()).map_err(ApiError::from)?;
    for (ty, folder) in &vault.index.type_folders {
        let index = vault.load_folder_index(folder).ok();
        folders.push(FolderSummaryResponse {
            r#type: ty.clone(),
            folder: folder.clone(),
            name: index
                .as_ref()
                .map(|index| index.name.clone())
                .or_else(|| Some(title_from_id(folder))),
            icon: kataan_core::types::TypeRegistry::load(&vault)
                .ok()
                .and_then(|registry| {
                    registry
                        .definitions
                        .get(ty)
                        .and_then(|definition| definition.icon.clone())
                }),
            document_count: recursive_filesystem_document_count(&state.vault_path.join(folder)),
        });
    }
    push_code_folder_if_needed(
        &state,
        vault
            .index
            .type_folders
            .values()
            .any(|folder| kataan_core::constants::is_code_folder(folder)),
        &mut folders,
    );

    Ok(Json(FoldersResponse { folders }))
}

pub async fn folder(
    State(state): State<AppState>,
    Path(folder): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let Ok(loaded) = read_loaded_vault(&state) else {
        return filesystem_folder_response(&state, &folder).map(Json);
    };
    let id = kataan_core::id::CanonicalId::parse(&folder)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let Some(record) = loaded.documents.get(&id) else {
        if kataan_core::constants::is_code_path(id.as_str()) {
            return Ok(Json(FolderResponse {
                folder,
                index: empty_code_folder_index(id.as_str()),
                documents: Vec::new(),
            }));
        }
        return Err(ApiError(anyhow::anyhow!(
            "folder `{folder}` does not exist"
        )));
    };
    let documents = direct_documents(&loaded, &id);
    let index = kataan_core::index::FolderIndex {
        name: document_name(record).unwrap_or_else(|| title_from_id(&folder)),
        description: None,
        default_type: Some(record.metadata.r#type.clone()),
        folder_checksum: None,
        documents: Vec::new(),
        subfolders: Vec::new(),
    };

    Ok(Json(FolderResponse {
        folder,
        index,
        documents,
    }))
}

pub async fn folder_by_id(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> Result<Json<CanonicalFolderResponse>, ApiError> {
    canonical_folder_response(&state, &query.id).map(Json)
}

pub async fn document_by_id(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> Result<Json<DocumentResponse>, ApiError> {
    document_response(&state, &query.id).map(Json)
}

pub async fn file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileResponse>, ApiError> {
    file_response(&state, &query.path).map(Json)
}

pub async fn highlight_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<HighlightResponse>, ApiError> {
    highlight_response(&state, &query.path, query.theme.as_deref()).map(Json)
}

pub async fn raw_file_by_path(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<axum::response::Response, ApiError> {
    raw_file_response(&state, &query.path)
}

pub async fn resolve_route(
    State(state): State<AppState>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let loaded = read_loaded_vault(&state)?;
    let id = loaded
        .resolve_route_token(&query.r#type, &query.token)
        .cloned()
        .ok_or_else(|| {
            ApiError(anyhow::anyhow!(
                "route token `{}` for type `{}` does not resolve",
                query.token,
                query.r#type
            ))
        })?;
    let document = loaded
        .documents
        .get(&id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("document `{id}` does not exist")))?;
    Ok(Json(ResolveResponse {
        id: id.as_str().to_owned(),
        folder: id.containing_folder().to_owned(),
        type_folder: id.top_level_folder().to_owned(),
        route_token: kataan_core::vault::route_token_for_id(&id),
        is_folder_index: document.is_folder_index,
    }))
}

pub async fn document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DocumentResponse>, ApiError> {
    document_response(&state, &id).map(Json)
}

pub async fn schema(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> Result<Json<kataan_core::schema::TomlSchemaResponse>, ApiError> {
    let loaded = read_loaded_vault(&state).ok();
    let response = kataan_core::schema::schema_response(&kind, loaded.as_ref())
        .ok_or_else(|| ApiError(anyhow::anyhow!("unknown schema kind `{kind}`")))?;
    Ok(Json(response))
}

pub async fn validate(State(state): State<AppState>) -> Result<Json<ValidateResponse>, ApiError> {
    debug!(vault = %state.vault_path.display(), "validating vault");
    let report =
        kataan_core::validate::validate(state.vault_path.as_ref()).map_err(ApiError::from)?;
    let ok = report.is_ok();
    let diagnostics = report
        .diagnostics
        .into_iter()
        .map(|diagnostic| DiagnosticResponse {
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            code: diagnostic.code,
            message: diagnostic.message,
            path: diagnostic.path,
        })
        .collect();

    match state.reload() {
        Ok(()) => debug!(vault = %state.vault_path.display(), "reloaded vault after validation"),
        Err(error) => {
            debug!(vault = %state.vault_path.display(), error = %error, "vault reload after validation skipped")
        }
    }

    Ok(Json(ValidateResponse { ok, diagnostics }))
}

pub async fn rebuild_indexes(State(state): State<AppState>) -> Result<Json<OkResponse>, ApiError> {
    info!(vault = %state.vault_path.display(), "rebuilding indexes");
    kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref()).map_err(ApiError::from)?;
    state.reload().map_err(ApiError::from)?;
    info!(vault = %state.vault_path.display(), "reloaded vault after rebuild");
    Ok(Json(OkResponse { ok: true }))
}

fn file_response(state: &AppState, path: &str) -> Result<FileResponse, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let kind = file_kind(file.extension.as_deref()).to_owned();
    let content = match kind.as_str() {
        "html" | "json" | "text" => read_text_file(&file.full_path)?,
        _ => String::new(),
    };

    Ok(FileResponse {
        path: path.to_owned(),
        name: file.name,
        extension: file.extension,
        kind,
        content,
    })
}

fn raw_file_response(state: &AppState, path: &str) -> Result<axum::response::Response, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let content_type = match file.extension.as_deref() {
        Some("svg") => "image/svg+xml",
        _ => {
            return Err(ApiError(anyhow::anyhow!(
                "file `{path}` cannot be previewed as an image"
            )))
        }
    };
    let bytes = std::fs::read(&file.full_path).map_err(|source| {
        ApiError(
            kataan_core::Error::Io {
                path: file.full_path.clone(),
                source,
            }
            .into(),
        )
    })?;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    Ok((headers, bytes).into_response())
}

fn highlight_response(
    state: &AppState,
    path: &str,
    theme_preference: Option<&str>,
) -> Result<HighlightResponse, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let (language_name, language) = highlight_language(file.extension.as_deref())
        .ok_or_else(|| ApiError(anyhow::anyhow!("file `{path}` cannot be highlighted")))?;
    let content = read_text_file(&file.full_path)?;
    let theme = lumis::themes::get(highlight_theme(theme_preference))
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let formatter = lumis::HtmlInlineBuilder::new()
        .language(language)
        .theme(Some(theme))
        .pre_class(Some("highlight-preview".to_owned()))
        .build()
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let highlighted_content = content.trim_end_matches(['\r', '\n']);
    let html = normalize_lumis_line_html(&lumis::highlight(highlighted_content, formatter));

    Ok(HighlightResponse {
        path: path.to_owned(),
        name: file.name,
        extension: file.extension,
        language: language_name.to_owned(),
        html,
    })
}

struct ResolvedVaultFile {
    full_path: std::path::PathBuf,
    name: String,
    extension: Option<String>,
}

fn resolve_vault_file(state: &AppState, path: &str) -> Result<ResolvedVaultFile, ApiError> {
    let relative = std::path::Path::new(path);
    if !is_safe_relative_path(relative) {
        return Err(ApiError(anyhow::anyhow!("invalid file path `{path}`")));
    }
    let full_path = regular_descendant_file_path(state.vault_path.as_ref(), relative)
        .ok_or_else(|| ApiError(anyhow::anyhow!("file `{path}` does not exist")))?;
    if crate::ignore::VaultIgnore::load(state.vault_path.as_ref())?.should_ignore_path(&full_path) {
        return Err(ApiError(anyhow::anyhow!("file `{path}` is ignored")));
    }
    let extension = full_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_owned);
    let name = full_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned();

    Ok(ResolvedVaultFile {
        full_path,
        name,
        extension,
    })
}

fn is_safe_relative_path(relative: &std::path::Path) -> bool {
    !relative.is_absolute()
        && relative.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn regular_descendant_file_path(
    root: &std::path::Path,
    relative: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut current = root.to_path_buf();
    let mut components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .peekable();

    components.peek()?;

    while let Some(component) = components.next() {
        current.push(component);
        let file_type = std::fs::symlink_metadata(&current).ok()?.file_type();
        if file_type.is_symlink() {
            return None;
        }
        if components.peek().is_some() {
            if !file_type.is_dir() {
                return None;
            }
        } else if !file_type.is_file() {
            return None;
        }
    }

    Some(current)
}

fn is_regular_file(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn is_regular_dir(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

fn read_text_file(path: &std::path::Path) -> Result<String, ApiError> {
    if !is_regular_file(path) {
        return Err(ApiError(anyhow::anyhow!(
            "file `{}` does not exist",
            path.display()
        )));
    }
    std::fs::read_to_string(path).map_err(|source| {
        ApiError(
            kataan_core::Error::Io {
                path: path.to_path_buf(),
                source,
            }
            .into(),
        )
    })
}

fn file_kind(extension: Option<&str>) -> &'static str {
    match extension {
        Some("html") | Some("htm") => "html",
        Some("json") => "json",
        Some("svg") => "image",
        Some(
            "md" | "txt" | "toml" | "rs" | "ts" | "js" | "sh" | "bash" | "yaml" | "yml" | "py",
        ) => "text",
        _ => "unsupported",
    }
}

fn normalize_lumis_line_html(html: &str) -> String {
    html.replace("\r\n</div>", "</div>")
        .replace("\n</div>", "</div>")
}

fn render_markdown_html(
    markdown: &str,
    base_folder: Option<&str>,
    theme_preference: Option<&str>,
) -> Result<String, ApiError> {
    use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut events = Vec::new();
    let mut parser = Parser::new_ext(markdown, options);

    while let Some(event) = parser.next() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language_hint = match kind {
                    CodeBlockKind::Fenced(language) => Some(language.to_string()),
                    CodeBlockKind::Indented => None,
                };
                let mut code = String::new();
                for code_event in parser.by_ref() {
                    match code_event {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(text) => code.push_str(&text),
                        Event::Code(text) => code.push_str(&text),
                        Event::SoftBreak | Event::HardBreak => code.push('\n'),
                        _ => {}
                    }
                }
                events.push(Event::Html(CowStr::Boxed(
                    render_code_block_html(&code, language_hint.as_deref(), theme_preference)?
                        .into_boxed_str(),
                )));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => events.push(Event::Start(Tag::Image {
                link_type,
                dest_url: rewrite_markdown_svg_url(&dest_url, base_folder)
                    .map(CowStr::Boxed)
                    .unwrap_or(dest_url),
                title,
                id,
            })),
            other => events.push(other),
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Ok(html)
}

fn rewrite_markdown_svg_url(dest_url: &str, base_folder: Option<&str>) -> Option<Box<str>> {
    if !is_local_markdown_url(dest_url) {
        return None;
    }

    let (path_part, query, fragment) = split_markdown_url(dest_url);
    if !path_part.to_ascii_lowercase().ends_with(".svg") {
        return None;
    }

    let vault_relative_path = normalize_markdown_asset_path(base_folder.unwrap_or(""), path_part)?;
    let mut rewritten = format!(
        "/api/file/raw?path={}",
        percent_encode_query_value(&vault_relative_path)
    );
    if let Some(query) = query {
        rewritten.push('&');
        rewritten.push_str(query);
    }
    if let Some(fragment) = fragment {
        rewritten.push('#');
        rewritten.push_str(fragment);
    }
    Some(rewritten.into_boxed_str())
}

fn is_local_markdown_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.starts_with('#') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.split_once(':').map(|(scheme, _)| scheme),
        Some("http" | "https" | "data" | "mailto" | "tel" | "javascript")
    ) {
        return false;
    }
    true
}

fn split_markdown_url(url: &str) -> (&str, Option<&str>, Option<&str>) {
    let without_fragment;
    let fragment;
    if let Some((head, tail)) = url.split_once('#') {
        without_fragment = head;
        fragment = Some(tail);
    } else {
        without_fragment = url;
        fragment = None;
    }

    if let Some((path, query)) = without_fragment.split_once('?') {
        (path, Some(query), fragment)
    } else {
        (without_fragment, None, fragment)
    }
}

fn normalize_markdown_asset_path(base_folder: &str, path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let mut parts = Vec::new();

    if !path.starts_with('/') {
        for part in base_folder.split('/') {
            if !part.is_empty() {
                parts.push(part.to_owned());
            }
        }
    }

    for part in path.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part.to_owned()),
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn percent_encode_query_value(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn render_code_block_html(
    code: &str,
    language_hint: Option<&str>,
    theme_preference: Option<&str>,
) -> Result<String, ApiError> {
    let Some((_, language)) = highlight_language_hint(language_hint) else {
        let class = language_hint
            .filter(|language| !language.trim().is_empty())
            .map(|language| format!(" class=\"language-{}\"", escape_html_attr(language.trim())))
            .unwrap_or_default();
        return Ok(format!(
            "<pre class=\"highlight-preview\"><code{class}>{}</code></pre>",
            escape_html(code)
        ));
    };

    let theme = lumis::themes::get(highlight_theme(theme_preference))
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let formatter = lumis::HtmlInlineBuilder::new()
        .language(language)
        .theme(Some(theme))
        .pre_class(Some("highlight-preview".to_owned()))
        .build()
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    Ok(normalize_lumis_line_html(&lumis::highlight(
        code.trim_end_matches(['\r', '\n']),
        formatter,
    )))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}

fn highlight_theme(theme_preference: Option<&str>) -> &'static str {
    match theme_preference {
        Some("light") => "catppuccin_latte",
        _ => "catppuccin_mocha",
    }
}

fn highlight_language(
    extension: Option<&str>,
) -> Option<(&'static str, lumis::languages::Language)> {
    highlight_language_name(extension)
}

fn highlight_language_hint(
    language_hint: Option<&str>,
) -> Option<(&'static str, lumis::languages::Language)> {
    let language = language_hint?.split_whitespace().next()?.trim();
    highlight_language_name(Some(language))
}

fn highlight_language_name(
    name: Option<&str>,
) -> Option<(&'static str, lumis::languages::Language)> {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("c") | Some("h") => Some(("c", lumis::languages::Language::C)),
        Some("cc") | Some("cpp") | Some("cxx") | Some("c++") | Some("hpp") | Some("hxx") => {
            Some(("cpp", lumis::languages::Language::CPlusPlus))
        }
        Some("hs") | Some("haskell") => Some(("haskell", lumis::languages::Language::Haskell)),
        Some("json") => Some(("json", lumis::languages::Language::JSON)),
        Some("toml") => Some(("toml", lumis::languages::Language::Toml)),
        Some("md") | Some("markdown") => Some(("markdown", lumis::languages::Language::Markdown)),
        Some("rs") | Some("rust") => Some(("rust", lumis::languages::Language::Rust)),
        Some("ts") | Some("typescript") => {
            Some(("typescript", lumis::languages::Language::TypeScript))
        }
        Some("js") | Some("javascript") => {
            Some(("javascript", lumis::languages::Language::JavaScript))
        }
        Some("sh") | Some("bash") => Some(("bash", lumis::languages::Language::Bash)),
        Some("yaml") | Some("yml") => Some(("yaml", lumis::languages::Language::YAML)),
        Some("py") | Some("python") => Some(("python", lumis::languages::Language::Python)),
        _ => None,
    }
}

fn document_response(state: &AppState, id: &str) -> Result<DocumentResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;

    if let Ok(loaded) = read_loaded_vault(state) {
        let record = loaded
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError(anyhow::anyhow!("document `{id}` does not exist")))?;
        return document_response_from_parts(&id, record.metadata, &record.markdown_path);
    }

    filesystem_document_response(state, &id)
}

fn canonical_folder_response(
    state: &AppState,
    id: &str,
) -> Result<CanonicalFolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let Ok(loaded) = read_loaded_vault(state) else {
        return filesystem_canonical_folder_response(state, &id);
    };
    let (record, folders, documents, markdown_path) = {
        let Some(record) = loaded.documents.get(&id).cloned() else {
            if kataan_core::constants::is_code_path(id.as_str()) {
                return Ok(CanonicalFolderResponse {
                    id: id.as_str().to_owned(),
                    metadata: None,
                    markdown: None,
                    folders: direct_code_folders(state, id.as_str())?,
                    documents: Vec::new(),
                    files: folder_files(state, &id, &[])?,
                });
            }
            return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
        };
        if !record.is_folder_index {
            return Err(ApiError(anyhow::anyhow!("document `{id}` is not a folder")));
        }
        let folders = direct_folders(&loaded, &id);
        let documents = direct_documents(&loaded, &id);
        (record.clone(), folders, documents, record.markdown_path)
    };
    let markdown = read_text_file(&markdown_path).ok();
    let files = folder_files(state, &id, &documents)?;

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata: Some(record.metadata),
        markdown,
        folders,
        documents,
        files,
    })
}

fn filesystem_document_response(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
) -> Result<DocumentResponse, ApiError> {
    let toml_path = state.vault_path.join(id.toml_path());
    let metadata = read_document_metadata_if_valid(&toml_path).ok_or_else(|| {
        ApiError(anyhow::anyhow!(
            "document `{id}` cannot be loaded because its TOML metadata is invalid"
        ))
    })?;
    let markdown_path = state.vault_path.join(id.folder()).join(&metadata.markdown);
    document_response_from_parts(id, metadata, &markdown_path)
}

fn document_response_from_parts(
    id: &kataan_core::id::CanonicalId,
    metadata: kataan_core::document::DocumentMetadata,
    markdown_path: &std::path::Path,
) -> Result<DocumentResponse, ApiError> {
    let markdown = read_text_file(markdown_path)?;

    let base_folder = if metadata.markdown == "index.md" {
        id.as_str()
    } else {
        id.folder()
    };
    let html = render_markdown_html(&markdown, Some(base_folder), None)?;

    Ok(DocumentResponse {
        id: id.as_str().to_owned(),
        type_folder: id.top_level_folder().to_owned(),
        route_token: kataan_core::vault::route_token_for_id(id),
        metadata,
        markdown,
        html,
    })
}

fn filesystem_folder_response(state: &AppState, folder: &str) -> Result<FolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(folder)
        .map_err(|source| ApiError(anyhow::anyhow!(source)))?;
    let response = filesystem_canonical_folder_response(state, &id)?;
    Ok(FolderResponse {
        folder: response.id.clone(),
        index: kataan_core::index::FolderIndex {
            name: response
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.aliases.first().cloned())
                .unwrap_or_else(|| title_from_id(&response.id)),
            description: None,
            default_type: response.metadata.map(|metadata| metadata.r#type),
            folder_checksum: None,
            documents: Vec::new(),
            subfolders: Vec::new(),
        },
        documents: response.documents,
    })
}

fn filesystem_canonical_folder_response(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
) -> Result<CanonicalFolderResponse, ApiError> {
    if kataan_core::constants::is_code_path(id.as_str()) {
        return Ok(CanonicalFolderResponse {
            id: id.as_str().to_owned(),
            metadata: None,
            markdown: None,
            folders: direct_code_folders(state, id.as_str())?,
            documents: Vec::new(),
            files: folder_files(state, id, &[])?,
        });
    }

    let folder_path = state.vault_path.join(id.as_str());
    if !is_regular_dir(&folder_path) {
        return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
    }

    let metadata = read_document_metadata_if_valid(&folder_path.join("index.toml"));
    let markdown = read_text_file(&folder_path.join("index.md")).ok();
    let mut folders = Vec::new();
    let mut documents = Vec::new();
    let ignore = crate::ignore::VaultIgnore::load(state.vault_path.as_ref())?;

    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        if ignore.should_ignore_path(&path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_regular_dir(&path) {
            folders.push(FolderChildResponse {
                id: format!("{}/{}", id.as_str(), name),
                name,
                has_index: is_regular_file(&path.join("index.md"))
                    && is_regular_file(&path.join("index.toml")),
            });
            continue;
        }
        if !is_regular_file(&path)
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || name == "index.md"
        {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{slug}.toml"));
        if read_document_metadata_if_valid(&toml_path).is_none() {
            continue;
        }
        documents.push(FolderDocumentResponse {
            id: format!("{}/{}", id.as_str(), slug),
            slug: slug.to_owned(),
            markdown: name,
            toml: format!("{slug}.toml"),
        });
    }

    folders.sort_by(|left, right| left.id.cmp(&right.id));
    documents.sort_by(|left, right| left.id.cmp(&right.id));
    let files = folder_files(state, id, &documents)?;

    Ok(CanonicalFolderResponse {
        id: id.as_str().to_owned(),
        metadata,
        markdown,
        folders,
        documents,
        files,
    })
}

fn folder_files(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
    documents: &[FolderDocumentResponse],
) -> Result<Vec<FolderFileResponse>, ApiError> {
    let folder_path = state.vault_path.join(id.as_str());
    let document_sidecars = documents
        .iter()
        .flat_map(|document| [document.markdown.as_str(), document.toml.as_str()])
        .collect::<std::collections::BTreeSet<_>>();
    let mut files = Vec::new();
    let ignore = crate::ignore::VaultIgnore::load(state.vault_path.as_ref())?;

    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        if ignore.should_ignore_path(&path) || !is_regular_file(&path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "index.md" || name == "index.toml" || document_sidecars.contains(name.as_str()) {
            continue;
        }
        files.push(FolderFileResponse {
            extension: path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_owned),
            path: format!("{}/{}", id.as_str(), name),
            name,
        });
    }

    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

fn read_document_metadata_if_valid(
    path: &std::path::Path,
) -> Option<kataan_core::document::DocumentMetadata> {
    let text = read_text_file(path).ok()?;
    toml::from_str(&text).ok()
}

fn push_code_folder_if_needed(
    state: &AppState,
    has_code_mapping: bool,
    folders: &mut Vec<FolderSummaryResponse>,
) {
    if !has_code_mapping
        && is_regular_dir(&state.vault_path.join(kataan_core::constants::CODE_FOLDER))
    {
        folders.push(FolderSummaryResponse {
            r#type: kataan_core::constants::TYPE_CODE.to_owned(),
            folder: kataan_core::constants::CODE_FOLDER.to_owned(),
            name: Some("Code".to_owned()),
            icon: Some("Code".to_owned()),
            document_count: 0,
        });
    }
}

fn empty_code_folder_index(id: &str) -> kataan_core::index::FolderIndex {
    kataan_core::index::FolderIndex {
        name: title_from_id(id),
        description: Some("Agent tools and code assets.".to_owned()),
        default_type: Some("code".to_owned()),
        folder_checksum: None,
        documents: Vec::new(),
        subfolders: Vec::new(),
    }
}

fn direct_code_folders(state: &AppState, id: &str) -> Result<Vec<FolderChildResponse>, ApiError> {
    let folder_path = state.vault_path.join(id);
    if !is_regular_dir(&folder_path) {
        return Err(ApiError(anyhow::anyhow!("folder `{id}` does not exist")));
    }

    let ignore = crate::ignore::VaultIgnore::load(state.vault_path.as_ref())?;
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(&folder_path).map_err(|source| kataan_core::Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| kataan_core::Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        if ignore.should_ignore_path(&path) || !is_regular_dir(&path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        folders.push(FolderChildResponse {
            id: format!("{id}/{name}"),
            name,
            has_index: false,
        });
    }
    folders.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(folders)
}

fn read_loaded_vault(state: &AppState) -> Result<kataan_core::vault::LoadedVault, ApiError> {
    state
        .vault
        .read()
        .map_err(|_| ApiError(anyhow::anyhow!("vault lock poisoned")))
        .map(|vault| vault.clone())
}

fn recursive_document_count(loaded: &kataan_core::vault::LoadedVault, folder: &str) -> usize {
    let descendant_prefix = format!("{folder}/");
    loaded
        .documents
        .values()
        .filter(|document| {
            !document.is_folder_index
                && (document.id.containing_folder() == folder
                    || document
                        .id
                        .containing_folder()
                        .starts_with(&descendant_prefix))
        })
        .count()
}

fn recursive_filesystem_document_count(folder_path: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(folder_path) else {
        return 0;
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if is_regular_dir(&path) {
                return recursive_filesystem_document_count(&path);
            }
            if !is_regular_file(&path)
                || path.extension().and_then(|extension| extension.to_str()) != Some("md")
                || path.file_name().and_then(|name| name.to_str()) == Some("index.md")
            {
                return 0;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return 0;
            };
            is_regular_file(&path.with_file_name(format!("{stem}.toml"))) as usize
        })
        .sum()
}

fn direct_folders(
    loaded: &kataan_core::vault::LoadedVault,
    id: &kataan_core::id::CanonicalId,
) -> Vec<FolderChildResponse> {
    loaded
        .graph
        .children_of(id)
        .into_iter()
        .filter_map(|child_id| {
            let child = loaded.documents.get(&child_id)?;
            child.is_folder_index.then(|| FolderChildResponse {
                id: child_id.as_str().to_owned(),
                name: document_name(child).unwrap_or_else(|| title_from_id(child_id.as_str())),
                has_index: true,
            })
        })
        .collect()
}

fn direct_documents(
    loaded: &kataan_core::vault::LoadedVault,
    id: &kataan_core::id::CanonicalId,
) -> Vec<FolderDocumentResponse> {
    loaded
        .graph
        .children_of(id)
        .into_iter()
        .filter_map(|child_id| {
            let child = loaded.documents.get(&child_id)?;
            (!child.is_folder_index).then(|| FolderDocumentResponse {
                id: child_id.as_str().to_owned(),
                slug: child_id.slug().to_owned(),
                markdown: child
                    .markdown_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                toml: child
                    .toml_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}
fn document_name(document: &kataan_core::vault::DocumentRecord) -> Option<String> {
    document
        .metadata
        .aliases
        .first()
        .cloned()
        .or_else(|| document.metadata.labels.first().cloned())
}

#[derive(Debug)]
pub struct ApiError(anyhow::Error);

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        error!(error = %self.0, "api request failed");
        let body = Json(serde_json::json!({
            "ok": false,
            "error": self.0.to_string(),
        }));
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

#[cfg(test)]
mod tests {
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

        assert!(html.contains(
            "src=\"/api/file/raw?path=projects/airline-anchor/charts/look-to-book.svg\""
        ));
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

    async fn json_response<T: serde::de::DeserializeOwned>(
        response: axum::response::Response,
    ) -> T {
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
}
