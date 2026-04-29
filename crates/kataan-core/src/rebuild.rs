use std::{fs, path::Path};

use crate::{
    checksum,
    constants::{is_code_folder, VAULT_CONFIG_FILE},
    index::{FolderDocument, VaultConfig},
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
            path: root_index_path,
            source,
        })?;

    for (document_type, folder) in &root_index.type_folders {
        if is_code_folder(folder) {
            continue;
        }
        let folder_path = root.join(folder);
        if !folder_path.exists() {
            continue;
        }

        let folder_index_path = folder_path.join("index.toml");
        let existing_folder_index = fs::read_to_string(&folder_index_path).unwrap_or_default();
        let (name, description, default_type) =
            parse_folder_index_header(&existing_folder_index, folder);

        let mut documents = Vec::new();
        let mut markdown_paths = Vec::new();
        for entry in fs::read_dir(&folder_path).map_err(|source| Error::Io {
            path: folder_path.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                path: folder_path.clone(),
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

        let folder_checksum = checksum::folder_checksum(&documents);
        write_folder_index(
            &folder_index_path,
            document_type,
            &name,
            description.as_deref(),
            default_type.as_deref(),
            &folder_checksum,
            &documents,
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

fn parse_folder_index_header(text: &str, folder: &str) -> (String, Option<String>, Option<String>) {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return (title_case(folder), None, None);
    };
    let name = value
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| title_case(folder));
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

fn write_folder_index(
    path: &Path,
    document_type: &str,
    name: &str,
    description: Option<&str>,
    default_type: Option<&str>,
    folder_checksum: &str,
    documents: &[FolderDocument],
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

    write::atomic_write_string(path, &output)
}

fn title_case(value: &str) -> String {
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
        for folder in ["raw", "people", "notes", "topics", "type"] {
            fs::create_dir_all(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("index.md"), format!("# {folder}\n")).unwrap();
            fs::write(
                root.join(folder).join("index.toml"),
                format!(
                    "type = \"{}\"\nname = \"{folder}\"\nmarkdown = \"index.md\"\n",
                    match folder {
                        "raw" => "raw",
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

    fn write_root_index(root: &Path) {
        fs::write(root.join("ontology.toml"), "schema_version = \"0.1.0\"\n").unwrap();
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"schema_version = "0.1.0"
name = "Test Vault"
created_at = "2026-04-28T12:00:00Z"
updated_at = "2026-04-28T12:00:00Z"

[type_folders]
raw = "raw"
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
