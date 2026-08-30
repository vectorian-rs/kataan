use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{
    constants::{DEFAULT_MAX_FOLDER_DEPTH, TYPE_DEFINITION, VAULT_CONFIG_FILE},
    diagnostic::{Diagnostic, DiagnosticReport},
    diagnostic_codes as codes,
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
    for warning in ignore.warnings() {
        issues.push(
            Diagnostic::warning(codes::INVALID_SCAN_PATTERN, warning.clone())
                .with_path(VAULT_CONFIG_FILE),
        );
    }

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

    let type_registry = Some(TypeRegistry::load(vault)?);
    let mut known_document_types = BTreeMap::new();
    match vault.load_documents_with_ignore(&ignore) {
        Ok(documents) => {
            for document in documents {
                known_document_ids.insert(document.id.as_str().to_owned());
                known_document_types
                    .insert(document.id.as_str().to_owned(), document.metadata.r#type);
            }
        }
        // Swallowing this reported a clean vault while every read path — CLI,
        // server and MCP — failed outright on the same data. `validate` is the
        // tool meant to explain that, so it has to say so.
        Err(error) => issues.push(Diagnostic::error(
            codes::INVALID_VAULT_STRUCTURE,
            format!("vault cannot be loaded: {error}"),
        )),
    }

    if let Some(type_registry) = &type_registry {
        validate_type_registry(&mut issues, vault, type_registry);
    }

    for folder in vault.index.type_folders.values() {
        if !crate::index::is_safe_type_folder(folder) {
            issues.push(
                Diagnostic::error(
                    codes::UNSAFE_TYPE_FOLDER,
                    format!(
                        "type folder `{folder}` must be a relative path inside the vault; \
                         an absolute path or `..` would read and write outside it"
                    ),
                )
                .with_path(VAULT_CONFIG_FILE),
            );
            continue;
        }
        let folder_path = vault.root.join(folder);
        if !crate::walk::is_regular_dir(&folder_path) {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_REQUIRED_FOLDER,
                    "Required type folder is missing",
                )
                .with_path(folder),
            );
            continue;
        }

        // The top-level type folder is the depth-0 case of the same walk, so
        // it now goes through the same code path. A check added in one place
        // covers every document instead of half the vault.
        folder::validate_nested_folder_recursive(
            &mut issues,
            folder,
            &folder_path,
            &folder_path,
            max_folder_depth,
            type_registry.as_ref(),
            &vault.index.type_folders,
            &ignore,
            &mut known_document_ids,
            &mut known_document_types,
            &mut loaded_metadata,
        )?;
    }

    for (path, _, metadata) in loaded_metadata {
        if let Some(ontology) = &ontology {
            issues.extend(
                crate::ontology::validate_node_fields(ontology, &metadata, &known_document_types)
                    .into_iter()
                    .map(|diagnostic| diagnostic.with_path(path.clone())),
            );

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
