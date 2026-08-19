//! Per-folder and per-document validation.
//!
//! Carved out of the parent `validate` module for file-size hygiene. The
//! orchestration in `validate.rs` drives the `pub(super)` entry points here.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    checksum::{self, SubfolderChecksum},
    constants::{ACTOR_VALUES, STATUS_VALUES},
    diagnostic::Diagnostic,
    diagnostic_codes as codes,
    document::DocumentMetadata,
    id::CanonicalId,
    index::{FolderDocument, FolderIndex, FolderSubfolder},
    scan::ScanIgnore,
    types::TypeRegistry,
    Result,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_nested_folder_recursive(
    issues: &mut Vec<Diagnostic>,
    root_folder: &str,
    root_folder_path: &Path,
    folder_path: &Path,
    max_folder_depth: usize,
    type_registry: Option<&TypeRegistry>,
    type_folders: &BTreeMap<String, String>,
    ignore: &ScanIgnore,
    known_document_ids: &mut BTreeSet<String>,
    known_document_types: &mut BTreeMap<String, String>,
    loaded_metadata: &mut Vec<(String, String, DocumentMetadata)>,
) -> Result<()> {
    let folder_index = validate_folder_pair(issues, root_folder, root_folder_path, folder_path)?;

    let relative = relative_folder_path(root_folder, root_folder_path, folder_path);
    let mut markdown_slugs = BTreeSet::new();
    let mut toml_slugs = BTreeSet::new();
    let mut document_toml_files: Vec<(PathBuf, String)> = Vec::new();

    for entry in fs::read_dir(folder_path).map_err(|source| crate::Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| crate::Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();

        if crate::walk::is_regular_dir(&path) {
            if ignore.is_ignored(&path, true) {
                continue;
            }
            validate_nested_folder_recursive(
                issues,
                root_folder,
                root_folder_path,
                &path,
                max_folder_depth,
                type_registry,
                type_folders,
                ignore,
                known_document_ids,
                known_document_types,
                loaded_metadata,
            )?;
            continue;
        }

        if !crate::walk::is_regular_file(&path) {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
            continue;
        };
        if file_name == "index.toml" || file_name == "index.md" {
            continue;
        }

        let relative_path = Path::new(&relative).join(file_name);
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        match extension {
            "md" => {
                if CanonicalId::from_document_path(&relative_path).is_ok() {
                    markdown_slugs.insert(stem.to_owned());
                }
            }
            "toml" if CanonicalId::from_document_path(&relative_path).is_ok() => {
                toml_slugs.insert(stem.to_owned());
                document_toml_files.push((path.clone(), format!("{relative}/{file_name}")));
            }
            _ => {}
        }
    }

    document_toml_files.retain(|(path, _)| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| markdown_slugs.contains(stem))
    });

    if let Some(folder_index) = folder_index {
        validate_folder_index(
            issues,
            &relative,
            folder_path,
            &folder_index,
            &markdown_slugs,
            &toml_slugs,
            ignore,
        )?;
    }

    for (toml_path, relative_toml_path) in document_toml_files {
        validate_document_metadata(
            issues,
            root_folder,
            folder_path,
            &toml_path,
            &relative_toml_path,
            max_folder_depth,
            type_registry,
            type_folders,
            known_document_ids,
            known_document_types,
            loaded_metadata,
        )?;
    }

    Ok(())
}

fn validate_folder_pair(
    issues: &mut Vec<Diagnostic>,
    root_folder: &str,
    root_folder_path: &Path,
    folder_path: &Path,
) -> Result<Option<FolderIndex>> {
    let relative = relative_folder_path(root_folder, root_folder_path, folder_path);
    validate_optional_folder_index_pair(issues, folder_path, &relative)
}

pub(super) fn validate_optional_folder_index_pair(
    issues: &mut Vec<Diagnostic>,
    folder_path: &Path,
    relative: &str,
) -> Result<Option<FolderIndex>> {
    let folder_index_path = folder_path.join("index.toml");
    let folder_markdown_path = folder_path.join("index.md");
    match (folder_markdown_path.exists(), folder_index_path.exists()) {
        (true, true) => read_folder_index_with_diagnostic(issues, folder_path, relative),
        (true, false) => {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_FOLDER_INDEX,
                    "Folder has index.md but is missing index.toml",
                )
                .with_path(format!("{relative}/index.toml")),
            );
            Ok(None)
        }
        (false, true) => {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_MARKDOWN_FILE,
                    "Folder has index.toml but is missing index.md",
                )
                .with_path(format!("{relative}/index.md")),
            );
            read_folder_index_with_diagnostic(issues, folder_path, relative)
        }
        (false, false) => Ok(None),
    }
}

pub(super) fn validate_metadata_markdown_path(
    issues: &mut Vec<Diagnostic>,
    toml_path: &Path,
    relative_toml_path: &str,
    metadata: &DocumentMetadata,
) -> bool {
    let Some(stem) = toml_path.file_stem().and_then(|stem| stem.to_str()) else {
        return true;
    };
    let expected_markdown = format!("{stem}.md");
    if metadata.markdown == expected_markdown {
        return true;
    }

    issues.push(
        Diagnostic::error(
            codes::MARKDOWN_PATH_MISMATCH,
            format!(
                "metadata.markdown must point to `{expected_markdown}`, not `{}`",
                metadata.markdown
            ),
        )
        .with_path(relative_toml_path.to_owned()),
    );
    false
}

fn relative_folder_path(root_folder: &str, root_folder_path: &Path, folder_path: &Path) -> String {
    folder_path
        .strip_prefix(root_folder_path)
        .map(|path| Path::new(root_folder).join(path))
        .unwrap_or_else(|_| Path::new(root_folder).to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

#[allow(clippy::too_many_arguments)]
fn validate_document_metadata(
    issues: &mut Vec<Diagnostic>,
    root_folder: &str,
    folder_path: &Path,
    toml_path: &Path,
    relative_toml_path: &str,
    max_folder_depth: usize,
    type_registry: Option<&TypeRegistry>,
    type_folders: &BTreeMap<String, String>,
    known_document_ids: &mut BTreeSet<String>,
    known_document_types: &mut BTreeMap<String, String>,
    loaded_metadata: &mut Vec<(String, String, DocumentMetadata)>,
) -> Result<()> {
    let toml_text = fs::read_to_string(toml_path).map_err(|source| crate::Error::Io {
        path: toml_path.to_path_buf(),
        source,
    })?;
    let metadata: DocumentMetadata = match toml::from_str(&toml_text) {
        Ok(metadata) => metadata,
        Err(source) => {
            issues.push(
                Diagnostic::error(codes::INVALID_TOML, source.to_string())
                    .with_path(relative_toml_path.to_owned()),
            );
            return Ok(());
        }
    };

    let Some(document_id) = relative_toml_path.strip_suffix(".toml") else {
        return Ok(());
    };
    let document_id = document_id.to_owned();
    known_document_ids.insert(document_id.clone());
    known_document_types.insert(document_id.clone(), metadata.r#type.clone());
    loaded_metadata.push((
        relative_toml_path.to_owned(),
        document_id.clone(),
        metadata.clone(),
    ));

    if let Ok(id) = CanonicalId::parse(&document_id) {
        if id.depth_after_type_folder() > max_folder_depth {
            issues.push(
                Diagnostic::error(
                    codes::FOLDER_DEPTH_EXCEEDED,
                    format!("document depth exceeds max_folder_depth `{max_folder_depth}`"),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        }
    }

    if let Some(status) = &metadata.status {
        if !STATUS_VALUES.contains(&status.as_str()) {
            issues.push(
                Diagnostic::error(codes::INVALID_STATUS, format!("unknown status `{status}`"))
                    .with_path(relative_toml_path.to_owned()),
            );
        }
    }

    for (field, actor) in [
        ("created_by", metadata.created_by.as_deref()),
        ("last_updated_by", metadata.last_updated_by.as_deref()),
    ] {
        if let Some(actor) = actor {
            if !ACTOR_VALUES.contains(&actor) {
                issues.push(
                    Diagnostic::error(
                        codes::INVALID_ACTOR,
                        format!("{field} has unknown actor `{actor}`"),
                    )
                    .with_path(relative_toml_path.to_owned()),
                );
            }
        }
    }

    let expected_type_folder = type_registry
        .and_then(|registry| registry.folder_for(&metadata.r#type))
        .or_else(|| type_folders.get(&metadata.r#type).map(String::as_str));

    match expected_type_folder {
        Some(expected_folder) if expected_folder != root_folder => {
            issues.push(
                Diagnostic::error(
                    codes::TYPE_FOLDER_MISMATCH,
                    format!(
                        "document type `{}` belongs in `{expected_folder}`, not `{root_folder}`",
                        metadata.r#type
                    ),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        }
        Some(_) => {}
        None => {
            issues.push(
                Diagnostic::error(
                    codes::INVALID_TYPE,
                    format!("unknown type `{}`", metadata.r#type),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        }
    }

    if !validate_metadata_markdown_path(issues, toml_path, relative_toml_path, &metadata) {
        return Ok(());
    }
    let markdown_path = folder_path.join(&metadata.markdown);
    if markdown_path.exists() {
        if let Some(expected_checksum) = metadata.markdown_checksum {
            let actual_checksum = checksum::blake3_file(&markdown_path)?;
            if actual_checksum != expected_checksum {
                issues.push(
                    Diagnostic::error(
                        codes::CHECKSUM_MISMATCH,
                        "Markdown checksum does not match file contents",
                    )
                    .with_path(relative_toml_path.to_owned()),
                );
            }
        }
    }

    Ok(())
}

fn read_folder_index_with_diagnostic(
    issues: &mut Vec<Diagnostic>,
    folder_path: &Path,
    relative: &str,
) -> Result<Option<FolderIndex>> {
    let path = folder_path.join("index.toml");
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|source| crate::Error::Io {
        path: path.clone(),
        source,
    })?;
    match toml::from_str::<FolderIndex>(&text) {
        Ok(index) => Ok(Some(index)),
        Err(source) => {
            issues.push(
                Diagnostic::error(codes::INVALID_TOML, source.to_string())
                    .with_path(format!("{relative}/index.toml")),
            );
            Ok(None)
        }
    }
}

fn read_folder_index_if_present(folder_path: &Path) -> Result<Option<FolderIndex>> {
    let path = folder_path.join("index.toml");
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|source| crate::Error::Io {
        path: path.clone(),
        source,
    })?;
    Ok(toml::from_str::<FolderIndex>(&text).ok())
}

pub(super) fn validate_folder_index(
    issues: &mut Vec<Diagnostic>,
    folder: &str,
    folder_path: &Path,
    folder_index: &FolderIndex,
    markdown_slugs: &BTreeSet<String>,
    toml_slugs: &BTreeSet<String>,
    ignore: &ScanIgnore,
) -> Result<()> {
    let actual_slugs = markdown_slugs
        .intersection(toml_slugs)
        .cloned()
        .collect::<BTreeSet<_>>();
    let indexed_slugs = folder_index
        .documents
        .iter()
        .map(|document| document.slug.clone())
        .collect::<BTreeSet<_>>();

    for slug in actual_slugs.difference(&indexed_slugs) {
        issues.push(
            Diagnostic::warning(codes::INDEX_DRIFT, "Document is missing from folder index")
                .with_path(format!("{folder}/{slug}.md")),
        );
    }

    for _slug in indexed_slugs.difference(&actual_slugs) {
        issues.push(
            Diagnostic::warning(
                codes::INDEX_DRIFT,
                "Folder index contains a stale document entry",
            )
            .with_path(format!("{folder}/index.toml")),
        );
    }

    let actual_subfolders = actual_subfolders(folder_path, ignore)?;
    let indexed_subfolders = folder_index
        .subfolders
        .iter()
        .map(|subfolder| subfolder.name.clone())
        .collect::<BTreeSet<_>>();
    let actual_subfolder_names = actual_subfolders
        .iter()
        .map(|subfolder| subfolder.name.clone())
        .collect::<BTreeSet<_>>();

    for name in actual_subfolder_names.difference(&indexed_subfolders) {
        issues.push(
            Diagnostic::warning(codes::INDEX_DRIFT, "Subfolder is missing from folder index")
                .with_path(format!("{folder}/{name}/index.toml")),
        );
    }

    for name in indexed_subfolders.difference(&actual_subfolder_names) {
        issues.push(
            Diagnostic::warning(
                codes::INDEX_DRIFT,
                format!("Folder index contains stale subfolder entry `{name}`"),
            )
            .with_path(format!("{folder}/index.toml")),
        );
    }

    let mut actual_documents = Vec::new();
    for document in &folder_index.documents {
        let markdown_path = folder_path.join(&document.markdown);
        let toml_path = folder_path.join(&document.toml);
        if !markdown_path.exists() || !toml_path.exists() {
            continue;
        }

        let actual_markdown_checksum = checksum::blake3_file(&markdown_path)?;
        if actual_markdown_checksum != document.markdown_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder index Markdown checksum does not match file contents",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }

        let actual_toml_checksum = checksum::blake3_file(&toml_path)?;
        if actual_toml_checksum != document.toml_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder index TOML checksum does not match file contents",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }

        actual_documents.push(FolderDocument {
            slug: document.slug.clone(),
            markdown: document.markdown.clone(),
            toml: document.toml.clone(),
            markdown_checksum: actual_markdown_checksum,
            toml_checksum: actual_toml_checksum,
        });
    }

    if let Some(expected_folder_checksum) = &folder_index.folder_checksum {
        let checksum_subfolders = actual_subfolders
            .iter()
            .map(|subfolder| SubfolderChecksum {
                name: subfolder.name.clone(),
                folder_checksum: subfolder.folder_checksum.clone(),
            })
            .collect::<Vec<_>>();
        let actual_folder_checksum =
            checksum::folder_checksum_recursive(&actual_documents, &checksum_subfolders);
        if &actual_folder_checksum != expected_folder_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder checksum does not match indexed document checksums",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }
    }

    Ok(())
}

fn actual_subfolders(folder_path: &Path, ignore: &ScanIgnore) -> Result<Vec<FolderSubfolder>> {
    let mut subfolders = Vec::new();
    for entry in fs::read_dir(folder_path).map_err(|source| crate::Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| crate::Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !crate::walk::is_regular_dir(&path)
            || ignore.is_ignored(&path, true)
            || !path.join("index.md").exists()
            || !path.join("index.toml").exists()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let index = read_folder_index_if_present(&path)?;
        if let Some(folder_checksum) = index.and_then(|index| index.folder_checksum) {
            subfolders.push(FolderSubfolder {
                name,
                folder_checksum,
            });
        }
    }
    subfolders.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(subfolders)
}
