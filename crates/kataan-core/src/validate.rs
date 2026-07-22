use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    checksum::{self, SubfolderChecksum},
    constants::{
        ACTOR_VALUES, DEFAULT_MAX_FOLDER_DEPTH, STATUS_VALUES, TYPE_DEFINITION, VAULT_CONFIG_FILE,
    },
    diagnostic::{Diagnostic, DiagnosticReport},
    diagnostic_codes as codes,
    document::DocumentMetadata,
    id::CanonicalId,
    index::{FolderDocument, FolderIndex, FolderSubfolder},
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

fn validate_type_registry(issues: &mut Vec<Diagnostic>, vault: &Vault, registry: &TypeRegistry) {
    for (ty, folder) in &vault.index.type_folders {
        let Some(definition) = registry.definitions.get(ty) else {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_FOLDER,
                    format!("type `{ty}` is mapped to `{folder}` but has no type definition"),
                )
                .with_path(VAULT_CONFIG_FILE),
            );
            continue;
        };

        let definition_md_path = vault.root.join("type").join(format!("{ty}.md"));
        if !definition_md_path.exists() {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_MARKDOWN_FILE,
                    format!("type definition `{ty}` is missing Markdown file"),
                )
                .with_path(format!("type/{ty}.toml")),
            );
        }

        if definition.r#type != TYPE_DEFINITION {
            issues.push(
                Diagnostic::error(
                    codes::INVALID_TYPE,
                    format!("type definition `{ty}` must have type = \"{TYPE_DEFINITION}\""),
                )
                .with_path(format!("type/{ty}.toml")),
            );
        }

        if definition.name != *ty {
            issues.push(
                Diagnostic::error(
                    codes::INVALID_TYPE,
                    format!(
                        "type definition name `{}` does not match `{ty}`",
                        definition.name
                    ),
                )
                .with_path(format!("type/{ty}.toml")),
            );
        }

        if definition.folder != *folder {
            issues.push(
                Diagnostic::error(
                    codes::TYPE_FOLDER_MISMATCH,
                    format!(
                        "type definition `{ty}` maps to `{}`, but kataan.toml maps it to `{folder}`",
                        definition.folder
                    ),
                )
                .with_path(format!("type/{ty}.toml")),
            );
        }
    }

    for ty in registry.definitions.keys() {
        if !vault.index.type_folders.contains_key(ty) {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_FOLDER,
                    format!("type definition `{ty}` has no matching type_folders entry"),
                )
                .with_path(format!("type/{ty}.toml")),
            );
        }
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

    let type_registry = match TypeRegistry::load(vault) {
        Ok(registry) => Some(registry),
        Err(error) => return Err(error),
    };
    let mut known_document_types = BTreeMap::new();
    if let Ok(documents) = vault.load_documents() {
        for document in documents {
            known_document_ids.insert(document.id.as_str().to_owned());
            known_document_types.insert(document.id.as_str().to_owned(), document.metadata.r#type);
        }
    }

    if let Some(type_registry) = &type_registry {
        validate_type_registry(&mut issues, vault, type_registry);
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

        let folder_index = validate_optional_folder_index_pair(&mut issues, &folder_path, folder)?;

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

        document_toml_files.retain(|(path, _)| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| markdown_slugs.contains(stem))
        });

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
                    issues.push(
                        Diagnostic::error(codes::INVALID_TOML, source.to_string())
                            .with_path(relative_toml_path),
                    );
                    continue;
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

            if !validate_metadata_markdown_path(
                &mut issues,
                &toml_path,
                &relative_toml_path,
                &metadata,
            ) {
                continue;
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

fn validate_optional_folder_index_pair(
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

fn validate_metadata_markdown_path(
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

    let actual_subfolders = actual_subfolders(folder_path)?;
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

fn actual_subfolders(folder_path: &Path) -> Result<Vec<FolderSubfolder>> {
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
        if !path.is_dir() || !path.join("index.md").exists() || !path.join("index.toml").exists() {
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
        fs::remove_file(root.join("type/project.md")).unwrap();
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
    fn reports_missing_required_folder_but_allows_structural_type_folder() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report.diagnostics.iter().any(|diagnostic| diagnostic.code
            == codes::MISSING_REQUIRED_FOLDER
            && diagnostic.path.as_deref() == Some("intake")));
        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/index.toml")));
        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/index.md")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_incomplete_nested_folder_index_pair() {
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
        assert!(report.diagnostics.iter().all(|diagnostic| !matches!(
            diagnostic.path.as_deref(),
            Some("projects/company-x/internal/index.md")
                | Some("projects/company-x/internal/index.toml")
        )));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_invalid_folder_index_toml_as_diagnostics() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "name = [\n").unwrap();
        fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
        fs::write(root.join("projects/company-x/index.toml"), "name = [\n").unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        for path in ["projects/index.toml", "projects/company-x/index.toml"] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == codes::INVALID_TOML
                        && diagnostic.path.as_deref() == Some(path)),
                "missing invalid TOML diagnostic for {path}: {:#?}",
                report.diagnostics
            );
        }

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
status = "weird"
markdown = "bad-status.md"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(!report
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
    fn reports_markdown_metadata_path_mismatch() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
        fs::write(root.join("projects/overview.md"), "# Overview\n").unwrap();
        fs::write(
            root.join("projects/overview.toml"),
            "type = \"project\"\nmarkdown = \"other.md\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
        fs::write(
            root.join("projects/company-x/index.toml"),
            "name = \"Company X\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/company-x/deep.md"), "# Deep\n").unwrap();
        fs::write(
            root.join("projects/company-x/deep.toml"),
            "type = \"project\"\nmarkdown = \"other.md\"\n",
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        for path in ["projects/overview.toml", "projects/company-x/deep.toml"] {
            assert!(
                report.diagnostics.iter().any(|diagnostic| diagnostic.code
                    == codes::MARKDOWN_PATH_MISMATCH
                    && diagnostic.path.as_deref() == Some(path)),
                "missing markdown path mismatch for {path}: {:#?}",
                report.diagnostics
            );
        }

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

        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/kataan-redesign.md")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn treats_standalone_toml_as_file() {
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

        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/kataan-redesign.toml")));

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
    fn validates_custom_type_folders() {
        let root = unique_temp_dir();
        crate::init::init_vault(&root, "Test Vault").unwrap();
        fs::create_dir_all(root.join("articles")).unwrap();
        let mut config = fs::read_to_string(root.join(VAULT_CONFIG_FILE)).unwrap();
        config.push_str("article = \"articles\"\n");
        fs::write(root.join(VAULT_CONFIG_FILE), config).unwrap();
        fs::write(root.join("articles/index.md"), "# Articles\n").unwrap();
        fs::write(
            root.join("articles/index.toml"),
            "type = \"article\"\nname = \"Articles\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(root.join("articles/essay.md"), "# Essay\n").unwrap();
        fs::write(
            root.join("articles/essay.toml"),
            "type = \"article\"\nmarkdown = \"essay.md\"\n",
        )
        .unwrap();
        fs::write(root.join("type/article.md"), "# Article\n").unwrap();
        fs::write(
            root.join("type/article.toml"),
            "type = \"type-definition\"\nname = \"article\"\nfolder = \"articles\"\nicon = \"Newspaper\"\nmarkdown = \"article.md\"\n",
        )
        .unwrap();

        crate::rebuild::rebuild_indexes(&root).unwrap();
        let report = validate(&root).unwrap();

        assert!(report.is_ok(), "{:#?}", report.diagnostics);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_type_definition_for_mapped_type() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
intake = "intake"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == codes::MISSING_TYPE_FOLDER
                && diagnostic.message.contains("intake")));

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
to = ["intake", "note"]
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
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("type")).unwrap();
        fs::write(root.join("type/index.md"), "# Types\n").unwrap();
        fs::write(
            root.join("type/index.toml"),
            "type = \"type-definition\"\nname = \"Types\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        for (ty, folder) in [
            ("intake", "intake"),
            ("project", "projects"),
            ("person", "people"),
            ("note", "notes"),
            ("topic", "topics"),
            ("type-definition", "type"),
        ] {
            fs::write(
                root.join("type").join(format!("{ty}.md")),
                format!("# {ty}\n"),
            )
            .unwrap();
            fs::write(
                root.join("type").join(format!("{ty}.toml")),
                format!(
                    "type = \"type-definition\"\nname = \"{ty}\"\nfolder = \"{folder}\"\nmarkdown = \"{ty}.md\"\n"
                ),
            )
            .unwrap();
        }
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("validate")
    }
}
