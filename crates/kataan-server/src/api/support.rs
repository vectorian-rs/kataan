//! File access, path safety, and response-building helpers for the API.
//!
//! Carved out of the parent `api` module for file-size hygiene.

use super::render::*;
use super::*;

pub(super) const MAX_TEXT_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;
pub(super) const MAX_RAW_PREVIEW_BYTES: u64 = 50 * 1024 * 1024;

pub(super) fn ensure_preview_size(
    path: &str,
    full_path: &std::path::Path,
    limit: u64,
) -> Result<(), ApiError> {
    let size = std::fs::symlink_metadata(full_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if size > limit {
        return Err(ApiError::too_large(format!(
            "file `{path}` is {} and exceeds the {} preview limit",
            format_megabytes(size),
            format_megabytes(limit)
        )));
    }
    Ok(())
}

pub(super) fn format_megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

pub(super) fn file_response(state: &AppState, path: &str) -> Result<FileResponse, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let kind = file_kind(file.extension.as_deref()).to_owned();
    let content = match kind.as_str() {
        "html" | "json" | "text" => {
            ensure_preview_size(path, &file.full_path, MAX_TEXT_PREVIEW_BYTES)?;
            read_text_file(&file.full_path)?
        }
        "image" | "pdf" => {
            ensure_preview_size(path, &file.full_path, MAX_RAW_PREVIEW_BYTES)?;
            String::new()
        }
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

pub(super) fn raw_file_response(
    state: &AppState,
    path: &str,
) -> Result<axum::response::Response, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let content_type = match file.extension.as_deref() {
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        _ => {
            return Err(ApiError::bad_request(format!(
                "file `{path}` cannot be previewed as raw content"
            )))
        }
    };
    ensure_preview_size(path, &file.full_path, MAX_RAW_PREVIEW_BYTES)?;
    let bytes = std::fs::read(&file.full_path).map_err(|source| {
        ApiError::from(kataan_core::Error::Io {
            path: file.full_path.clone(),
            source,
        })
    })?;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    Ok((headers, bytes).into_response())
}

pub(super) fn highlight_response(
    state: &AppState,
    path: &str,
    theme_preference: Option<&str>,
) -> Result<HighlightResponse, ApiError> {
    let file = resolve_vault_file(state, path)?;
    let (language_name, language) = highlight_language_name(file.extension.as_deref())
        .ok_or_else(|| ApiError::bad_request(format!("file `{path}` cannot be highlighted")))?;
    ensure_preview_size(path, &file.full_path, MAX_TEXT_PREVIEW_BYTES)?;
    let content = read_text_file(&file.full_path)?;
    let html = highlight_to_html(&content, language, theme_preference)?;

    Ok(HighlightResponse {
        path: path.to_owned(),
        name: file.name,
        extension: file.extension,
        language: language_name.to_owned(),
        html,
    })
}

pub(super) struct ResolvedVaultFile {
    full_path: std::path::PathBuf,
    name: String,
    extension: Option<String>,
}

pub(super) fn resolve_vault_file(
    state: &AppState,
    path: &str,
) -> Result<ResolvedVaultFile, ApiError> {
    let relative = std::path::Path::new(path);
    if !is_safe_relative_path(relative) {
        return Err(ApiError::bad_request(format!("invalid file path `{path}`")));
    }
    let full_path = regular_descendant_file_path(state.vault_path.as_ref(), relative)
        .ok_or_else(|| ApiError::not_found(format!("file `{path}` does not exist")))?;
    if state.ignore().should_ignore_path(&full_path) {
        return Err(ApiError::not_found(format!("file `{path}` is ignored")));
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

pub(super) fn is_safe_relative_path(relative: &std::path::Path) -> bool {
    !relative.is_absolute()
        && relative.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

pub(super) fn regular_descendant_file_path(
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

pub(super) fn is_regular_file(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

pub(super) fn is_regular_dir(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

pub(super) fn read_dir_entries(dir: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, ApiError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|source| kataan_core::Error::Io {
        path: dir.to_path_buf(),
        source,
    })? {
        entries.push(entry.map_err(|source| kataan_core::Error::Io {
            path: dir.to_path_buf(),
            source,
        })?);
    }
    Ok(entries)
}

pub(super) fn read_text_file(path: &std::path::Path) -> Result<String, ApiError> {
    if !is_regular_file(path) {
        return Err(ApiError::not_found(format!(
            "file `{}` does not exist",
            path.display()
        )));
    }
    std::fs::read_to_string(path).map_err(|source| {
        ApiError::from(kataan_core::Error::Io {
            path: path.to_path_buf(),
            source,
        })
    })
}

pub(super) fn file_kind(extension: Option<&str>) -> &'static str {
    match extension {
        Some("html") | Some("htm") => "html",
        Some("json") => "json",
        Some("svg") => "image",
        Some("pdf") => "pdf",
        Some(
            "md" | "txt" | "toml" | "rs" | "ts" | "js" | "sh" | "bash" | "yaml" | "yml" | "py",
        ) => "text",
        _ => "unsupported",
    }
}

pub(super) fn document_response(state: &AppState, id: &str) -> Result<DocumentResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id).map_err(ApiError::bad_request)?;

    if let Ok(loaded) = read_loaded_vault(state) {
        let record = loaded
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("document `{id}` does not exist")))?;
        return document_response_from_parts(&id, record.metadata, &record.markdown_path);
    }

    filesystem_document_response(state, &id)
}

pub(super) fn canonical_folder_response(
    state: &AppState,
    id: &str,
) -> Result<CanonicalFolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(id).map_err(ApiError::bad_request)?;
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
            return Err(ApiError::not_found(format!("folder `{id}` does not exist")));
        };
        if !record.is_folder_index {
            return Err(ApiError::bad_request(format!(
                "document `{id}` is not a folder"
            )));
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

pub(super) fn filesystem_document_response(
    state: &AppState,
    id: &kataan_core::id::CanonicalId,
) -> Result<DocumentResponse, ApiError> {
    let toml_path = state.vault_path.join(id.toml_path());
    let metadata = read_document_metadata_if_valid(&toml_path).ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "document `{id}` cannot be loaded because its TOML metadata is invalid"
        ))
    })?;
    let markdown_path = state.vault_path.join(id.folder()).join(&metadata.markdown);
    document_response_from_parts(id, metadata, &markdown_path)
}

pub(super) fn document_response_from_parts(
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

pub(super) fn filesystem_folder_response(
    state: &AppState,
    folder: &str,
) -> Result<FolderResponse, ApiError> {
    let id = kataan_core::id::CanonicalId::parse(folder).map_err(ApiError::bad_request)?;
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

pub(super) fn filesystem_canonical_folder_response(
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
        return Err(ApiError::not_found(format!("folder `{id}` does not exist")));
    }

    let metadata = read_document_metadata_if_valid(&folder_path.join("index.toml"));
    let markdown = read_text_file(&folder_path.join("index.md")).ok();
    let mut folders = Vec::new();
    let mut documents = Vec::new();
    let ignore = state.ignore();

    for entry in read_dir_entries(&folder_path)? {
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

pub(super) fn folder_files(
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
    let ignore = state.ignore();

    for entry in read_dir_entries(&folder_path)? {
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

pub(super) fn read_document_metadata_if_valid(
    path: &std::path::Path,
) -> Option<kataan_core::document::DocumentMetadata> {
    let text = read_text_file(path).ok()?;
    toml::from_str(&text).ok()
}

pub(super) fn push_code_folder_if_needed(
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

pub(super) fn empty_code_folder_index(id: &str) -> kataan_core::index::FolderIndex {
    kataan_core::index::FolderIndex {
        name: title_from_id(id),
        description: Some("Agent tools and code assets.".to_owned()),
        default_type: Some("code".to_owned()),
        folder_checksum: None,
        documents: Vec::new(),
        subfolders: Vec::new(),
    }
}

pub(super) fn direct_code_folders(
    state: &AppState,
    id: &str,
) -> Result<Vec<FolderChildResponse>, ApiError> {
    let folder_path = state.vault_path.join(id);
    if !is_regular_dir(&folder_path) {
        return Err(ApiError::not_found(format!("folder `{id}` does not exist")));
    }

    let ignore = state.ignore();
    let mut folders = Vec::new();
    for entry in read_dir_entries(&folder_path)? {
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

pub(super) fn read_loaded_vault(
    state: &AppState,
) -> Result<std::sync::Arc<kataan_core::vault::LoadedVault>, ApiError> {
    state
        .vault
        .read()
        .map_err(|_| ApiError::from(anyhow::anyhow!("vault lock poisoned")))
        .map(|vault| std::sync::Arc::clone(&vault))
}

pub(super) fn recursive_document_count(
    loaded: &kataan_core::vault::LoadedVault,
    folder: &str,
) -> usize {
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

pub(super) fn recursive_filesystem_document_count(folder_path: &std::path::Path) -> usize {
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

pub(super) fn direct_folders(
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

pub(super) fn direct_documents(
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
pub(super) fn document_name(document: &kataan_core::vault::DocumentRecord) -> Option<String> {
    kataan_core::document::display_name(&document.metadata)
}
