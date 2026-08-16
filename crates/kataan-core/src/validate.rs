use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{
    checksum,
    constants::{
        ACTOR_VALUES, DEFAULT_MAX_FOLDER_DEPTH, STATUS_VALUES, TYPE_DEFINITION, VAULT_CONFIG_FILE,
    },
    diagnostic::{Diagnostic, DiagnosticReport},
    diagnostic_codes as codes,
    document::DocumentMetadata,
    id::CanonicalId,
    ontology::{type_allowed, Ontology},
    scan::ScanIgnore,
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
    let ignore = ScanIgnore::load(&vault.root, &vault.index.scan)?;

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

        let folder_index =
            folder::validate_optional_folder_index_pair(&mut issues, &folder_path, folder)?;

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
                if ignore.is_ignored(&path, true) {
                    continue;
                }
                folder::validate_nested_folder_recursive(
                    &mut issues,
                    folder,
                    &folder_path,
                    &path,
                    max_folder_depth,
                    type_registry.as_ref(),
                    &vault.index.type_folders,
                    &ignore,
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
            folder::validate_folder_index(
                &mut issues,
                folder,
                &folder_path,
                folder_index,
                &markdown_slugs,
                &toml_slugs,
                &ignore,
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

            if !folder::validate_metadata_markdown_path(
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

mod folder;

#[cfg(test)]
mod tests;
