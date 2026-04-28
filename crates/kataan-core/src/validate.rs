use std::{collections::BTreeSet, fs, path::Path};

use crate::{
    checksum,
    diagnostic::{Diagnostic, DiagnosticReport},
    document::DocumentMetadata,
    id::CanonicalId,
    vault::Vault,
    Result,
};

pub fn validate(root: impl AsRef<Path>) -> Result<DiagnosticReport> {
    let vault = Vault::open(root)?;
    let mut issues = Vec::new();
    let mut known_document_ids = BTreeSet::new();
    let mut loaded_metadata = Vec::new();

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
        if !folder_index_path.exists() {
            issues.push(
                Diagnostic::error("missing-folder-index", "Folder is missing index.toml")
                    .with_path(format!("{folder}/index.toml")),
            );
        }

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
            if file_name == "index.toml" {
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
            loaded_metadata.push((relative_toml_path.clone(), metadata.clone()));

            if let Some(status) = &metadata.status {
                if !["raw", "draft", "active", "paused", "done", "archived"]
                    .contains(&status.as_str())
                {
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

            match vault.index.type_folders.get(&metadata.r#type) {
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

    for (path, metadata) in loaded_metadata {
        for target in metadata
            .belongs_to
            .iter()
            .chain(metadata.related_to.iter())
            .chain(metadata.sources.iter())
        {
            if !known_document_ids.contains(target) {
                issues.push(
                    Diagnostic::error(
                        "unresolved-reference",
                        format!("reference target `{target}` does not exist"),
                    )
                    .with_path(path.clone()),
                );
            }
        }
    }

    Ok(DiagnosticReport::new(issues))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;

    #[test]
    fn reports_unresolved_relationship_references() {
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
related_to = ["topics/missing-topic"]
belongs_to = ["projects/missing-parent"]
sources = ["raw/missing-source"]
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        let unresolved_count = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "unresolved-reference")
            .count();
        assert_eq!(unresolved_count, 3);

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
