use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{
    checksum,
    diagnostic::{Diagnostic, DiagnosticReport},
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
    let mut issues = Vec::new();
    let mut known_document_ids = BTreeSet::new();
    let mut loaded_metadata = Vec::new();
    let max_folder_depth = vault.index.limits.max_folder_depth.unwrap_or(4);

    let ontology = match Ontology::load(&vault.root) {
        Ok(ontology) => {
            issues.extend(ontology.validate());
            Some(ontology)
        }
        Err(crate::Error::Io { .. }) => {
            issues.push(
                Diagnostic::error("missing-ontology", "vault is missing ontology.toml")
                    .with_path("ontology.toml"),
            );
            None
        }
        Err(error) => return Err(error),
    };

    let type_registry = TypeRegistry::load(&vault).ok();
    let mut known_document_types = BTreeMap::new();
    if let Ok(documents) = vault.load_documents() {
        for document in documents {
            known_document_ids.insert(document.id.as_str().to_owned());
            known_document_types.insert(document.id.as_str().to_owned(), document.metadata.r#type);
        }
    }

    for required_type in [
        "raw",
        "project",
        "person",
        "note",
        "topic",
        "type-definition",
    ] {
        if !vault.index.type_folders.contains_key(required_type) {
            issues.push(
                Diagnostic::error(
                    "missing-type-folder",
                    format!("missing type_folders entry for `{required_type}`"),
                )
                .with_path("index.toml"),
            );
        }
    }

    for folder in vault.index.type_folders.values() {
        let folder_path = vault.root.join(folder);
        if !folder_path.exists() {
            issues.push(
                Diagnostic::error("missing-required-folder", "Required type folder is missing")
                    .with_path(folder),
            );
            continue;
        }

        let folder_index_path = folder_path.join("index.toml");
        let folder_index = if !folder_index_path.exists() {
            issues.push(
                Diagnostic::error("missing-folder-index", "Folder is missing index.toml")
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
                    "missing-toml-sidecar",
                    "Markdown file is missing a matching TOML sidecar",
                )
                .with_path(format!("{folder}/{slug}.md")),
            );
        }

        for slug in toml_slugs.difference(&markdown_slugs) {
            issues.push(
                Diagnostic::error(
                    "missing-markdown-file",
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
                            "folder-depth-exceeded",
                            format!("document depth exceeds max_folder_depth `{max_folder_depth}`"),
                        )
                        .with_path(relative_toml_path.clone()),
                    );
                }
            }

            if let Some(status) = &metadata.status {
                if !["draft", "active", "paused", "done", "archived"].contains(&status.as_str()) {
                    issues.push(
                        Diagnostic::error("invalid-status", format!("unknown status `{status}`"))
                            .with_path(relative_toml_path.clone()),
                    );
                }
            }

            for (field, actor) in [
                ("created_by", metadata.created_by.as_deref()),
                ("last_updated_by", metadata.last_updated_by.as_deref()),
            ] {
                if let Some(actor) = actor {
                    if !["human", "agent", "system"].contains(&actor) {
                        issues.push(
                            Diagnostic::error(
                                "invalid-actor",
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
                            "type-folder-mismatch",
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
                            "invalid-type",
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
                            "checksum-mismatch",
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
                            "unknown-predicate",
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
                            "predicate-source-type-mismatch",
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
                                "unresolved-edge-target",
                                format!("edge target `{target}` does not exist"),
                            )
                            .with_path(path.clone()),
                        );
                        continue;
                    };

                    if !type_allowed(&predicate.to, target_type) {
                        issues.push(
                            Diagnostic::error(
                                "predicate-target-type-mismatch",
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
            Diagnostic::warning("index-drift", "Document is missing from folder index")
                .with_path(format!("{folder}/{slug}.md")),
        );
    }

    for _slug in indexed_slugs.difference(&actual_slugs) {
        issues.push(
            Diagnostic::warning(
                "index-drift",
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
                    "checksum-mismatch",
                    "Folder index Markdown checksum does not match file contents",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }

        let actual_toml_checksum = checksum::blake3_file(&toml_path)?;
        if actual_toml_checksum != document.toml_checksum {
            issues.push(
                Diagnostic::error(
                    "checksum-mismatch",
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
                    "checksum-mismatch",
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
            .any(|diagnostic| diagnostic.code == "checksum-mismatch"
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
            .any(|diagnostic| diagnostic.code == "index-drift"));

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
            .any(|diagnostic| diagnostic.code == "missing-markdown-file"
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
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-required-folder"
                && diagnostic.path.as_deref() == Some("raw")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-folder-index"
                && diagnostic.path.as_deref() == Some("projects/index.toml")));

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
            .any(|diagnostic| diagnostic.code == "invalid-status"));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "invalid-actor"));

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
            .any(|diagnostic| diagnostic.code == "type-folder-mismatch"
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
            .any(|diagnostic| diagnostic.code == "checksum-mismatch"
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
            .any(|diagnostic| diagnostic.code == "missing-toml-sidecar"
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
            .any(|diagnostic| diagnostic.code == "missing-markdown-file"
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
            "unknown-predicate",
            "predicate-target-type-mismatch",
            "unresolved-edge-target",
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
            root.join("index.toml"),
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
            .any(|diagnostic| diagnostic.code == "missing-type-folder"
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
            root.join("index.toml"),
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
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("kataan-test-{}-{counter}", std::process::id()))
    }
}
