use std::{fs, path::Path};

use crate::{
    checksum::{self, SubfolderChecksum},
    constants::VAULT_CONFIG_FILE,
    index::{FolderDocument, FolderSubfolder, VaultConfig},
    write, Error, Result,
};

pub fn rebuild_indexes(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    let root_index_path = root.join(VAULT_CONFIG_FILE);
    let root_index_text = fs::read_to_string(&root_index_path).map_err(|source| Error::Io {
        path: root_index_path.clone(),
        source,
    })?;
    let root_index: VaultConfig =
        toml::from_str(&root_index_text).map_err(|source| Error::TomlParse {
            path: root_index_path.clone(),
            source,
        })?;

    for (document_type, folder) in &root_index.type_folders {
        let folder_path = root.join(folder);
        if folder_path.exists() {
            rebuild_folder_recursive(&folder_path, document_type)?;
        }
    }

    update_root_updated_at(&root_index_path, &root_index_text)?;

    Ok(())
}

fn rebuild_folder_recursive(folder_path: &Path, document_type: &str) -> Result<Option<String>> {
    let mut subfolders = Vec::new();
    for entry in fs::read_dir(folder_path).map_err(|source| Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(folder_checksum) = rebuild_folder_recursive(&path, document_type)? {
            subfolders.push(FolderSubfolder {
                name,
                folder_checksum,
            });
        }
    }
    subfolders.sort_by(|left, right| left.name.cmp(&right.name));

    let folder_index_path = folder_path.join("index.toml");
    let folder_markdown_path = folder_path.join("index.md");
    let has_index_pair = folder_index_path.exists() && folder_markdown_path.exists();
    let existing_folder_index = fs::read_to_string(&folder_index_path).unwrap_or_default();
    let (name, description, default_type) =
        parse_folder_index_header(&existing_folder_index, folder_path);

    let mut markdown_paths = Vec::new();
    for entry in fs::read_dir(folder_path).map_err(|source| Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || path.file_name().and_then(|name| name.to_str()) == Some("index.md")
        {
            continue;
        }
        markdown_paths.push(path);
    }
    markdown_paths.sort();

    let mut documents = Vec::new();
    for markdown_path in markdown_paths {
        let Some(slug) = markdown_path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{slug}.toml"));
        if !toml_path.exists() {
            continue;
        }

        let markdown_file_name = format!("{slug}.md");
        let toml_file_name = format!("{slug}.toml");
        let markdown_checksum = checksum::blake3_file(&markdown_path)?;
        update_document_markdown_checksum(&toml_path, &markdown_file_name, &markdown_checksum)?;
        let toml_checksum = checksum::blake3_file(&toml_path)?;
        documents.push(FolderDocument {
            slug: slug.to_owned(),
            markdown: markdown_file_name,
            toml: toml_file_name,
            markdown_checksum,
            toml_checksum,
        });
    }
    documents.sort_by(|left, right| left.slug.cmp(&right.slug));

    if !has_index_pair && documents.is_empty() && subfolders.is_empty() {
        return Ok(None);
    }
    ensure_folder_index_pair(
        &folder_markdown_path,
        &folder_index_path,
        document_type,
        &name,
        default_type.as_deref(),
    )?;

    let checksum_subfolders = subfolders
        .iter()
        .map(|subfolder| SubfolderChecksum {
            name: subfolder.name.clone(),
            folder_checksum: subfolder.folder_checksum.clone(),
        })
        .collect::<Vec<_>>();
    let folder_checksum = checksum::folder_checksum_recursive(&documents, &checksum_subfolders);
    write_folder_index(
        &folder_index_path,
        document_type,
        &name,
        description.as_deref(),
        default_type.as_deref(),
        &folder_checksum,
        &documents,
        &subfolders,
    )?;

    Ok(Some(folder_checksum))
}

fn ensure_folder_index_pair(
    markdown_path: &Path,
    toml_path: &Path,
    document_type: &str,
    name: &str,
    default_type: Option<&str>,
) -> Result<()> {
    if !markdown_path.exists() {
        write::atomic_write_string(markdown_path, &format!("# {name}\n"))?;
    }
    if !toml_path.exists() {
        let default_type = default_type.unwrap_or(document_type);
        write::atomic_write_string(
            toml_path,
            &format!(
                "type = \"{document_type}\"\nmarkdown = \"index.md\"\nname = \"{name}\"\ndefault_type = \"{default_type}\"\n"
            ),
        )?;
    }
    Ok(())
}

fn update_document_markdown_checksum(
    toml_path: &Path,
    markdown_file_name: &str,
    markdown_checksum: &str,
) -> Result<()> {
    let text = fs::read_to_string(toml_path).map_err(|source| Error::Io {
        path: toml_path.to_path_buf(),
        source,
    })?;
    let mut value: toml::Value = toml::from_str(&text).map_err(|source| Error::TomlParse {
        path: toml_path.to_path_buf(),
        source,
    })?;

    let table = value
        .as_table_mut()
        .expect("document TOML root must be table");
    table.insert(
        "markdown".to_owned(),
        toml::Value::String(markdown_file_name.to_owned()),
    );
    table.insert(
        "markdown_checksum".to_owned(),
        toml::Value::String(markdown_checksum.to_owned()),
    );

    let updated = toml::to_string_pretty(&value).expect("serialize document TOML");
    write::atomic_write_string(toml_path, &updated)
}

fn update_root_updated_at(path: &Path, text: &str) -> Result<()> {
    let mut value: toml::Value = toml::from_str(text).map_err(|source| Error::TomlParse {
        path: path.to_path_buf(),
        source,
    })?;
    let table = value.as_table_mut().expect("vault TOML root must be table");
    table.insert(
        "updated_at".to_owned(),
        toml::Value::String(unix_timestamp_string()),
    );
    let updated = toml::to_string_pretty(&value).expect("serialize vault TOML");
    write::atomic_write_string(path, &updated)
}

fn unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

fn parse_folder_index_header(
    text: &str,
    folder_path: &Path,
) -> (String, Option<String>, Option<String>) {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return (title_case(folder_path), None, None);
    };
    let name = value
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| title_case(folder_path));
    let description = value
        .get("description")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let default_type = value
        .get("default_type")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    (name, description, default_type)
}

#[allow(clippy::too_many_arguments)]
fn write_folder_index(
    path: &Path,
    document_type: &str,
    name: &str,
    description: Option<&str>,
    default_type: Option<&str>,
    folder_checksum: &str,
    documents: &[FolderDocument],
    subfolders: &[FolderSubfolder],
) -> Result<()> {
    let mut output = String::new();
    output.push_str(&format!("type = \"{document_type}\"\n"));
    output.push_str("markdown = \"index.md\"\n");
    output.push_str(&format!("name = \"{name}\"\n"));
    if let Some(description) = description {
        output.push_str(&format!("description = \"{description}\"\n"));
    }
    if let Some(default_type) = default_type {
        output.push_str(&format!("default_type = \"{default_type}\"\n"));
    }
    output.push_str(&format!("folder_checksum = \"{folder_checksum}\"\n"));

    for document in documents {
        output.push_str("\n[[documents]]\n");
        output.push_str(&format!("slug = \"{}\"\n", document.slug));
        output.push_str(&format!("markdown = \"{}\"\n", document.markdown));
        output.push_str(&format!("toml = \"{}\"\n", document.toml));
        output.push_str(&format!(
            "markdown_checksum = \"{}\"\n",
            document.markdown_checksum
        ));
        output.push_str(&format!("toml_checksum = \"{}\"\n", document.toml_checksum));
    }

    for subfolder in subfolders {
        output.push_str("\n[[subfolders]]\n");
        output.push_str(&format!("name = \"{}\"\n", subfolder.name));
        output.push_str(&format!(
            "folder_checksum = \"{}\"\n",
            subfolder.folder_checksum
        ));
    }

    write::atomic_write_string(path, &output)
}

fn title_case(path: &Path) -> String {
    let value = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("folder");
    let spaced = value.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use crate::{checksum, validate::validate};

    use super::*;

    #[test]
    fn rebuilds_document_checksums_and_folder_index() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::create_dir_all(root.join("code")).unwrap();
        for folder in ["intake", "people", "notes", "topics", "type"] {
            fs::create_dir_all(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("index.md"), format!("# {folder}\n")).unwrap();
            fs::write(
                root.join(folder).join("index.toml"),
                format!(
                    "type = \"{}\"\nname = \"{folder}\"\nmarkdown = \"index.md\"\n",
                    match folder {
                        "intake" => "intake",
                        "people" => "person",
                        "notes" => "note",
                        "topics" => "topic",
                        "type" => "type-definition",
                        _ => "project",
                    }
                ),
            )
            .unwrap();
        }
        for (ty, folder) in [
            ("intake", "intake"),
            ("project", "projects"),
            ("person", "people"),
            ("note", "notes"),
            ("topic", "topics"),
            ("code", "code"),
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

        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            "type = \"project\"\nname = \"Projects\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
status = "active"
markdown = "kataan-redesign.md"
created_by = "human"
last_updated_by = "human"
"#,
        )
        .unwrap();

        rebuild_indexes(&root).unwrap();

        let document_toml = fs::read_to_string(root.join("projects/kataan-redesign.toml")).unwrap();
        let expected_markdown_checksum =
            checksum::blake3_file(root.join("projects/kataan-redesign.md")).unwrap();
        assert!(document_toml.contains(&format!(
            "markdown_checksum = \"{expected_markdown_checksum}\""
        )));

        let folder_index = fs::read_to_string(root.join("projects/index.toml")).unwrap();
        assert!(folder_index.contains("[[documents]]"));
        assert!(folder_index.contains("slug = \"kataan-redesign\""));
        assert!(folder_index.contains("markdown = \"kataan-redesign.md\""));
        assert!(folder_index.contains("toml = \"kataan-redesign.toml\""));
        assert!(folder_index.contains("folder_checksum = \"blake3:"));

        let report = validate(&root).unwrap();
        assert!(report.is_ok(), "{:#?}", report.diagnostics);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebuilds_nested_subfolders_post_order() {
        let root = unique_temp_dir();
        crate::init::init_vault(&root, "Nested Vault").unwrap();
        fs::create_dir_all(root.join("projects/snappy/sows")).unwrap();
        fs::write(root.join("projects/snappy/index.md"), "# Snappy\n").unwrap();
        fs::write(
            root.join("projects/snappy/index.toml"),
            "type = \"project\"\nname = \"Snappy\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/snappy/sows/index.md"), "# SOWs\n").unwrap();
        fs::write(
            root.join("projects/snappy/sows/index.toml"),
            "type = \"project\"\nname = \"SOWs\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/snappy/sows/demo.md"), "# Demo\n").unwrap();
        fs::write(
            root.join("projects/snappy/sows/demo.toml"),
            "type = \"project\"\nmarkdown = \"demo.md\"\n",
        )
        .unwrap();

        rebuild_indexes(&root).unwrap();

        let projects_index = fs::read_to_string(root.join("projects/index.toml")).unwrap();
        let snappy_index = fs::read_to_string(root.join("projects/snappy/index.toml")).unwrap();
        let sows_index = fs::read_to_string(root.join("projects/snappy/sows/index.toml")).unwrap();

        assert!(projects_index.contains("[[subfolders]]"));
        assert!(projects_index.contains("name = \"snappy\""));
        assert!(snappy_index.contains("[[subfolders]]"));
        assert!(snappy_index.contains("name = \"sows\""));
        assert!(sows_index.contains("[[documents]]"));
        assert!(sows_index.contains("slug = \"demo\""));

        let report = validate(&root).unwrap();
        assert!(report.is_ok(), "{:#?}", report.diagnostics);

        fs::remove_dir_all(root).unwrap();
    }

    fn write_root_index(root: &Path) {
        fs::write(root.join("ontology.toml"), "schema_version = \"0.1.0\"\n").unwrap();
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"schema_version = "0.1.0"
name = "Test Vault"
created_at = "2026-04-28T12:00:00Z"
updated_at = "2026-04-28T12:00:00Z"

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
code = "code"
type-definition = "type"
"#,
        )
        .unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("rebuild")
    }
}
