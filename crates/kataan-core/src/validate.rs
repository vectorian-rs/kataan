use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    checksum,
    constants::{
        ACTOR_VALUES, CORE_TYPES, DEFAULT_MAX_FOLDER_DEPTH, STATUS_VALUES, VAULT_CONFIG_FILE,
    },
    diagnostic::{Diagnostic, DiagnosticReport},
    diagnostic_codes as codes,
    document::DocumentMetadata,
    id::CanonicalId,
    index::{FolderDocument, FolderIndex},
    ontology::{type_allowed, Ontology},
    types::TypeRegistry,
    vault::Vault,
    Result,
};

pub fn validate(root: impl AsRef<Path>) -> Result<DiagnosticReport> {
    let vault = Vault::open(root)?;
    Validator::new(vault).validate()
}

pub struct Validator {
    vault: Vault,
}

impl Validator {
    pub fn new(vault: Vault) -> Self {
        Self { vault }
    }

    pub fn validate(&self) -> Result<DiagnosticReport> {
        validate_open_vault(&self.vault)
    }
}

fn validate_open_vault(vault: &Vault) -> Result<DiagnosticReport> {
    let mut issues = Vec::new();
    let mut known_document_ids = BTreeSet::new();
    let mut loaded_metadata = Vec::new();
    let max_folder_depth = vault
        .index
        .limits
        .max_folder_depth
        .unwrap_or(DEFAULT_MAX_FOLDER_DEPTH);

    let ontology = match Ontology::load(&vault.root) {
        Ok(ontology) => {
            issues.extend(ontology.validate());
            Some(ontology)
        }
        Err(crate::Error::Io { .. }) => {
            issues.push(
                Diagnostic::error(codes::MISSING_ONTOLOGY, "vault is missing ontology.toml")
                    .with_path("ontology.toml"),
            );
            None
        }
        Err(error) => return Err(error),
    };

    let type_registry = TypeRegistry::load(vault).ok();
    let mut known_document_types = BTreeMap::new();
    if let Ok(documents) = vault.load_documents() {
        for document in documents {
            known_document_ids.insert(document.id.as_str().to_owned());
            known_document_types.insert(document.id.as_str().to_owned(), document.metadata.r#type);
        }
    }

    for required_type in CORE_TYPES {
        if !vault.index.type_folders.contains_key(*required_type) {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_FOLDER,
                    format!("missing type_folders entry for `{required_type}`"),
                )
                .with_path(VAULT_CONFIG_FILE),
            );
        }
    }

    for folder in vault.index.type_folders.values() {
        let folder_path = vault.root.join(folder);
        if !folder_path.exists() {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_REQUIRED_FOLDER,
                    "Required type folder is missing",
                )
                .with_path(folder),
            );
            continue;
        }

        let folder_index_path = folder_path.join("index.toml");
        let folder_markdown_path = folder_path.join("index.md");
        if !folder_markdown_path.exists() {
            issues.push(
                Diagnostic::error(codes::MISSING_MARKDOWN_FILE, "Folder is missing index.md")
                    .with_path(format!("{folder}/index.md")),
            );
        }
        let folder_index = if !folder_index_path.exists() {
            issues.push(
                Diagnostic::error(codes::MISSING_FOLDER_INDEX, "Folder is missing index.toml")
                    .with_path(format!("{folder}/index.toml")),
            );
            None
        } else {
            let index_text =
                fs::read_to_string(&folder_index_path).map_err(|source| crate::Error::Io {
                    path: folder_index_path.clone(),
                    source,
                })?;
            Some(
                toml::from_str::<FolderIndex>(&index_text).map_err(|source| {
                    crate::Error::TomlParse {
                        path: folder_index_path.clone(),
                        source,
                    }
                })?,
            )
        };

        let mut markdown_slugs = BTreeSet::new();
        let mut toml_slugs = BTreeSet::new();
        let mut document_toml_files = Vec::new();

        for entry in fs::read_dir(&folder_path).map_err(|source| crate::Error::Io {
            path: folder_path.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| crate::Error::Io {
                path: folder_path.clone(),
                source,
            })?;
            let path = entry.path();

            if path.is_dir() {
                validate_nested_folder_recursive(
                    &mut issues,
                    folder,
                    &folder_path,
                    &path,
                    max_folder_depth,
                    type_registry.as_ref(),
                    &vault.index.type_folders,
                    &mut known_document_ids,
                    &mut known_document_types,
                    &mut loaded_metadata,
                )?;
                continue;
            }

            if !path.is_file() {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
                continue;
            };
            if file_name == "index.toml" || file_name == "index.md" {
                continue;
            }

            let relative_path = Path::new(folder).join(file_name);
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
                "toml" => {
                    if CanonicalId::from_document_path(&relative_path).is_ok() {
                        toml_slugs.insert(stem.to_owned());
                        document_toml_files.push((path.clone(), format!("{folder}/{file_name}")));
                    }
                }
                _ => {}
            }
        }

        for slug in markdown_slugs.difference(&toml_slugs) {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_TOML_SIDECAR,
                    "Markdown file is missing a matching TOML sidecar",
                )
                .with_path(format!("{folder}/{slug}.md")),
            );
        }

        for slug in toml_slugs.difference(&markdown_slugs) {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_MARKDOWN_FILE,
                    "TOML sidecar is missing its Markdown file",
                )
                .with_path(format!("{folder}/{slug}.toml")),
            );
        }

        if let Some(folder_index) = &folder_index {
            validate_folder_index(
                &mut issues,
                folder,
                &folder_path,
                folder_index,
                &markdown_slugs,
                &toml_slugs,
            )?;
        }

        for (toml_path, relative_toml_path) in document_toml_files {
            let toml_text = fs::read_to_string(&toml_path).map_err(|source| crate::Error::Io {
                path: toml_path.clone(),
                source,
            })?;
            let metadata: DocumentMetadata = match toml::from_str(&toml_text) {
                Ok(metadata) => metadata,
                Err(source) => {
                    return Err(crate::Error::TomlParse {
                        path: toml_path,
                        source,
                    });
                }
            };

            let Some(stem) = toml_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let document_id = format!("{folder}/{stem}");
            known_document_ids.insert(document_id.clone());
            known_document_types.insert(document_id.clone(), metadata.r#type.clone());
            loaded_metadata.push((
                relative_toml_path.clone(),
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
                        .with_path(relative_toml_path.clone()),
                    );
                }
            }

            if let Some(status) = &metadata.status {
                if !STATUS_VALUES.contains(&status.as_str()) {
                    issues.push(
                        Diagnostic::error(
                            codes::INVALID_STATUS,
                            format!("unknown status `{status}`"),
                        )
                        .with_path(relative_toml_path.clone()),
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
                            .with_path(relative_toml_path.clone()),
                        );
                    }
                }
            }

            let expected_type_folder = type_registry
                .as_ref()
                .and_then(|registry| registry.folder_for(&metadata.r#type))
                .or_else(|| {
                    vault
                        .index
                        .type_folders
                        .get(&metadata.r#type)
                        .map(String::as_str)
                });

            match expected_type_folder {
                Some(expected_folder) if expected_folder != folder => {
                    issues.push(
                        Diagnostic::error(
                            codes::TYPE_FOLDER_MISMATCH,
                            format!(
                                "document type `{}` belongs in `{expected_folder}`, not `{folder}`",
                                metadata.r#type
                            ),
                        )
                        .with_path(relative_toml_path.clone()),
                    );
                }
                Some(_) => {}
                None => {
                    issues.push(
                        Diagnostic::error(
                            codes::INVALID_TYPE,
                            format!("unknown type `{}`", metadata.r#type),
                        )
                        .with_path(relative_toml_path.clone()),
                    );
                }
            }

            let markdown_path = folder_path.join(&metadata.markdown);
            if !markdown_path.exists() {
                continue;
            }

            if let Some(expected_checksum) = metadata.markdown_checksum {
                let actual_checksum = checksum::blake3_file(&markdown_path)?;
                if actual_checksum != expected_checksum {
                    issues.push(
                        Diagnostic::error(
                            codes::CHECKSUM_MISMATCH,
                            "Markdown checksum does not match file contents",
                        )
                        .with_path(relative_toml_path),
                    );
                }
            }
        }
    }

    for (path, _, metadata) in loaded_metadata {
        if let Some(ontology) = &ontology {
            for (predicate_name, targets) in &metadata.edges {
                let Some(predicate) = ontology.edges.get(predicate_name) else {
                    issues.push(
                        Diagnostic::error(
                            codes::UNKNOWN_PREDICATE,
                            format!(
                                "edge predicate `{predicate_name}` is not defined in ontology.toml"
                            ),
                        )
                        .with_path(path.clone()),
                    );
                    continue;
                };

                if !type_allowed(&predicate.from, &metadata.r#type) {
                    issues.push(
                        Diagnostic::error(
                            codes::PREDICATE_SOURCE_TYPE_MISMATCH,
                            format!(
                                "predicate `{predicate_name}` cannot be used from type `{}`",
                                metadata.r#type
                            ),
                        )
                        .with_path(path.clone()),
                    );
                }

                for target in targets {
                    let Some(target_type) = known_document_types.get(target) else {
                        issues.push(
                            Diagnostic::error(
                                codes::UNRESOLVED_EDGE_TARGET,
                                format!("edge target `{target}` does not exist"),
                            )
                            .with_path(path.clone()),
                        );
                        continue;
                    };

                    if !type_allowed(&predicate.to, target_type) {
                        issues.push(
                            Diagnostic::error(
                                codes::PREDICATE_TARGET_TYPE_MISMATCH,
                                format!(
                                    "predicate `{predicate_name}` cannot target `{target}` of type `{target_type}`"
                                ),
                            )
                            .with_path(path.clone()),
                        );
                    }
                }
            }
        }
    }

    Ok(DiagnosticReport::new(issues))
}

#[allow(clippy::too_many_arguments)]
fn validate_nested_folder_recursive(
    issues: &mut Vec<Diagnostic>,
    root_folder: &str,
    root_folder_path: &Path,
    folder_path: &Path,
    max_folder_depth: usize,
    type_registry: Option<&TypeRegistry>,
    type_folders: &BTreeMap<String, String>,
    known_document_ids: &mut BTreeSet<String>,
    known_document_types: &mut BTreeMap<String, String>,
    loaded_metadata: &mut Vec<(String, String, DocumentMetadata)>,
) -> Result<()> {
    validate_folder_pair(issues, root_folder, root_folder_path, folder_path)?;

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

        if path.is_dir() {
            validate_nested_folder_recursive(
                issues,
                root_folder,
                root_folder_path,
                &path,
                max_folder_depth,
                type_registry,
                type_folders,
                known_document_ids,
                known_document_types,
                loaded_metadata,
            )?;
            continue;
        }

        if !path.is_file() {
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
            "toml" => {
                if CanonicalId::from_document_path(&relative_path).is_ok() {
                    toml_slugs.insert(stem.to_owned());
                    document_toml_files.push((path.clone(), format!("{relative}/{file_name}")));
                }
            }
            _ => {}
        }
    }

    for slug in markdown_slugs.difference(&toml_slugs) {
        issues.push(
            Diagnostic::error(
                codes::MISSING_TOML_SIDECAR,
                "Markdown file is missing a matching TOML sidecar",
            )
            .with_path(format!("{relative}/{slug}.md")),
        );
    }

    for slug in toml_slugs.difference(&markdown_slugs) {
        issues.push(
            Diagnostic::error(
                codes::MISSING_MARKDOWN_FILE,
                "TOML sidecar is missing its Markdown file",
            )
            .with_path(format!("{relative}/{slug}.toml")),
        );
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
) -> Result<()> {
    let relative = relative_folder_path(root_folder, root_folder_path, folder_path);

    if !folder_path.join("index.md").exists() {
        issues.push(
            Diagnostic::error(codes::MISSING_MARKDOWN_FILE, "Folder is missing index.md")
                .with_path(format!("{relative}/index.md")),
        );
    }

    if !folder_path.join("index.toml").exists() {
        issues.push(
            Diagnostic::error(codes::MISSING_FOLDER_INDEX, "Folder is missing index.toml")
                .with_path(format!("{relative}/index.toml")),
        );
    }

    Ok(())
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
    let metadata: DocumentMetadata =
        toml::from_str(&toml_text).map_err(|source| crate::Error::TomlParse {
            path: toml_path.to_path_buf(),
            source,
        })?;

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

fn validate_folder_index(
    issues: &mut Vec<Diagnostic>,
    folder: &str,
    folder_path: &Path,
    folder_index: &FolderIndex,
    markdown_slugs: &BTreeSet<String>,
    toml_slugs: &BTreeSet<String>,
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
        let actual_folder_checksum = checksum::folder_checksum(&actual_documents);
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;

    #[test]
    fn reports_folder_index_checksum_mismatch() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();
        fs::write(
            root.join("projects/index.toml"),
            r#"name = "Projects"
default_type = "project"
folder_checksum = "blake3:not-real"

[[documents]]
slug = "kataan-redesign"
markdown = "kataan-redesign.md"
toml = "kataan-redesign.toml"
markdown_checksum = "blake3:not-real"
toml_checksum = "blake3:not-real"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::CHECKSUM_MISMATCH
                && diagnostic.path.as_deref() == Some("projects/index.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_folder_index_drift() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();
        fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();

        let report = validate(&root).unwrap();

        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::INDEX_DRIFT));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_type_definition_file() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("type")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("type/project.toml"),
            r#"type = "type-definition"
name = "project"
folder = "projects"
markdown = "project.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_MARKDOWN_FILE
                && diagnostic.path.as_deref() == Some("type/project.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_required_folder_and_folder_index() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report.diagnostics.iter().any(|diagnostic| diagnostic.code
            == codes::MISSING_REQUIRED_FOLDER
            && diagnostic.path.as_deref() == Some("raw")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_FOLDER_INDEX
                && diagnostic.path.as_deref() == Some("projects/index.toml")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_MARKDOWN_FILE
                && diagnostic.path.as_deref() == Some("projects/index.md")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_nested_folder_index_pair() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x/internal")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
        fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_FOLDER_INDEX
                && diagnostic.path.as_deref() == Some("projects/company-x/index.toml")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_MARKDOWN_FILE
                && diagnostic.path.as_deref() == Some("projects/company-x/internal/index.md")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_FOLDER_INDEX
                && diagnostic.path.as_deref() == Some("projects/company-x/internal/index.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validates_nested_documents_recursively() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
        fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
        fs::write(
            root.join("projects/company-x/index.toml"),
            "name = \"Company X\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/company-x/orphan.md"), "# Orphan\n").unwrap();
        fs::write(root.join("projects/company-x/bad-status.md"), "# Bad\n").unwrap();
        fs::write(
            root.join("projects/company-x/bad-status.toml"),
            r#"type = "project"
status = "raw"
markdown = "bad-status.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_TOML_SIDECAR
                && diagnostic.path.as_deref() == Some("projects/company-x/orphan.md")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::INVALID_STATUS
                && diagnostic.path.as_deref() == Some("projects/company-x/bad-status.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_invalid_status_and_actor() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"
type = "project"
status = "Active"
markdown = "kataan-redesign.md"
created_by = "robot"
last_updated_by = "agent"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::INVALID_STATUS));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::INVALID_ACTOR));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_type_folder_mismatch() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"
type = "note"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::TYPE_FOLDER_MISMATCH
                && diagnostic.path.as_deref() == Some("projects/kataan-redesign.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_markdown_checksum_mismatch() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"
type = "project"
markdown = "kataan-redesign.md"
markdown_checksum = "blake3:not-the-real-hash"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::CHECKSUM_MISMATCH
                && diagnostic.path.as_deref() == Some("projects/kataan-redesign.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_toml_sidecar() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_TOML_SIDECAR
                && diagnostic.path.as_deref() == Some("projects/kataan-redesign.md")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_markdown_file() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"
type = "project"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_MARKDOWN_FILE
                && diagnostic.path.as_deref() == Some("projects/kataan-redesign.toml")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_edge_and_ontology_errors() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/kataan-redesign.md"), "# Project\n").unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
markdown = "kataan-redesign.md"

[edges]
derived_from = ["projects/kataan-redesign"]
missing_predicate = ["notes/summary"]
related_to = ["notes/missing"]
"#,
        )
        .unwrap();
        fs::write(root.join("notes/summary.md"), "# Summary\n").unwrap();
        fs::write(
            root.join("notes/summary.toml"),
            r#"type = "note"
markdown = "summary.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        for code in [
            codes::UNKNOWN_PREDICATE,
            codes::PREDICATE_TARGET_TYPE_MISMATCH,
            codes::UNRESOLVED_EDGE_TARGET,
        ] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing {code}: {:#?}",
                report.diagnostics
            );
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_required_type_folder_entries() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
raw = "raw"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_TYPE_FOLDER
                && diagnostic.message.contains("project")));

        fs::remove_dir_all(root).unwrap();
    }

    fn write_root_index(root: &Path) {
        fs::write(
            root.join("ontology.toml"),
            r#"schema_version = "0.1.0"

[edges.related_to]
from = ["*"]
to = ["*"]
symmetric = true
cardinality = "many-to-many"

[edges.derived_from]
from = ["*"]
to = ["raw", "note"]
inverse = "derived"
cardinality = "many-to-many"
"#,
        )
        .unwrap();
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
raw = "raw"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#,
        )
        .unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("validate")
    }
}
