use std::{fs, path::Path};

use serde::Serialize;

use crate::{
    checksum::{self, SubfolderChecksum},
    constants::VAULT_CONFIG_FILE,
    index::{FolderDocument, FolderSubfolder, VaultConfig},
    scan::ScanIgnore,
    title::title_from_path,
    write, Error, Result,
};
#[derive(Serialize)]
struct FolderIndexToml<'a> {
    #[serde(rename = "type")]
    document_type: &'a str,
    markdown: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder_checksum: Option<&'a str>,
    /// Serialized before the arrays of tables so the emitted TOML keeps
    /// `[type_folders]` at the top level rather than trailing a `[[documents]]`
    /// block, where a reader would have to know TOML well to see it is not
    /// nested inside it.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    type_folders: &'a std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    documents: &'a [FolderDocument],
    #[serde(skip_serializing_if = "slice_is_empty")]
    subfolders: &'a [FolderSubfolder],
}

fn slice_is_empty<T>(slice: &&[T]) -> bool {
    slice.is_empty()
}

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

    let ignore = ScanIgnore::load(root, &root_index.scan)?;
    for (document_type, folder) in &root_index.type_folders {
        // Rebuild creates and rewrites files, so an unsafe type folder would
        // mutate paths outside the vault. Refuse rather than skip: unlike a
        // read, silently rebuilding part of a vault hides the problem.
        if !crate::index::is_safe_type_folder(folder) {
            return Err(Error::InvalidVaultStructure(format!(
                "type folder `{folder}` must be a relative path inside the vault"
            )));
        }
        let folder_path = root.join(folder);
        if crate::walk::is_regular_dir(&folder_path) {
            rebuild_folder_recursive(&folder_path, document_type, &ignore, 0)?;
        }
    }

    update_root_updated_at(&root_index_path, &root_index_text)?;

    Ok(())
}

fn rebuild_folder_recursive(
    folder_path: &Path,
    document_type: &str,
    ignore: &ScanIgnore,
    depth: usize,
) -> Result<Option<String>> {
    if depth > crate::constants::MAX_WALK_DEPTH {
        return Err(Error::InvalidVaultStructure(format!(
            "`{}` nests deeper than {} directories",
            folder_path.display(),
            crate::constants::MAX_WALK_DEPTH
        )));
    }
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
        if !crate::walk::is_regular_dir(&path) || ignore.is_ignored(&path, true) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(folder_checksum) =
            rebuild_folder_recursive(&path, document_type, ignore, depth + 1)?
        {
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
    let header = parse_folder_index_header(&existing_folder_index, folder_path);

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
        if !crate::walk::is_regular_file(&path)
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
        &header,
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
        &header,
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
    header: &FolderHeader,
) -> Result<()> {
    let name = header.name.as_str();
    if !markdown_path.exists() {
        write::atomic_write_string(markdown_path, &format!("# {name}\n"))?;
    }
    if !toml_path.exists() {
        let folder_index = FolderIndexToml {
            document_type: header.document_type.as_deref().unwrap_or(document_type),
            markdown: "index.md",
            name,
            description: None,
            default_type: Some(header.default_type.as_deref().unwrap_or(document_type)),
            folder_checksum: None,
            type_folders: &header.type_folders,
            documents: &[],
            subfolders: &[],
        };
        let folder_index_text =
            toml::to_string_pretty(&folder_index).expect("serialize folder index TOML");
        write::atomic_write_string(toml_path, &folder_index_text)?;
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
    // Strict write: whatever the file carried (older vaults wrote a bare Unix
    // epoch here), the value we emit is ISO-8601. Reads stay lenient, so an
    // un-rebuilt vault still loads — the format heals on the next rebuild
    // rather than needing a migration.
    table.insert(
        "updated_at".to_owned(),
        toml::Value::String(crate::time::iso8601_utc_now()),
    );
    let updated = toml::to_string_pretty(&value).expect("serialize vault TOML");
    write::atomic_write_string(path, &updated)
}

/// The parts of an existing `index.toml` that a rewrite must preserve.
///
/// Grouped rather than returned as a tuple because every new preserved field
/// would otherwise widen the tuple at four call sites, and a
/// `(String, Option<String>, Option<String>, ...)` gives the reader nothing to
/// check a mistaken argument order against.
struct FolderHeader {
    /// The type this folder already declares. A folder deeper in the tree may
    /// legitimately carry a type other than its top-level folder's, so the
    /// rewrite keeps what is there rather than stamping the root type over it.
    document_type: Option<String>,
    name: String,
    description: Option<String>,
    default_type: Option<String>,
    type_folders: std::collections::BTreeMap<String, String>,
}

fn parse_folder_index_header(text: &str, folder_path: &Path) -> FolderHeader {
    let fallback = |folder_path: &Path| FolderHeader {
        document_type: None,
        name: title_from_path(folder_path, "folder"),
        description: None,
        default_type: None,
        type_folders: std::collections::BTreeMap::new(),
    };
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return fallback(folder_path);
    };
    let name = value
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| title_from_path(folder_path, "folder"));
    let description = value
        .get("description")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let default_type = value
        .get("default_type")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    // Carried through the rewrite verbatim. `write_folder_index` regenerates
    // the whole file, so anything not read back here is silently dropped on
    // the next `rebuild-indexes`.
    let type_folders = value
        .get("type_folders")
        .and_then(toml::Value::as_table)
        .map(|table| {
            table
                .iter()
                .filter_map(|(ty, pattern)| {
                    pattern
                        .as_str()
                        .map(|pattern| (ty.clone(), pattern.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    FolderHeader {
        document_type: value
            .get("type")
            .and_then(toml::Value::as_str)
            .map(str::to_owned),
        name,
        description,
        default_type,
        type_folders,
    }
}

fn write_folder_index(
    path: &Path,
    document_type: &str,
    header: &FolderHeader,
    folder_checksum: &str,
    documents: &[FolderDocument],
    subfolders: &[FolderSubfolder],
) -> Result<()> {
    let folder_index = FolderIndexToml {
        document_type: header.document_type.as_deref().unwrap_or(document_type),
        markdown: "index.md",
        name: header.name.as_str(),
        description: header.description.as_deref(),
        default_type: header.default_type.as_deref(),
        folder_checksum: Some(folder_checksum),
        type_folders: &header.type_folders,
        documents,
        subfolders,
    };
    let output = toml::to_string_pretty(&folder_index).expect("serialize folder index TOML");
    write::atomic_write_string(path, &output)
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
    fn rebuild_serializes_folder_index_strings() {
        let root = unique_temp_dir();
        crate::init::init_vault(&root, "Rebuild Vault").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            "type = \"project\"\nmarkdown = \"index.md\"\nname = \"Projects \\\"Quoted\\\"\"\ndescription = \"Line one\\nLine two\"\n",
        )
        .unwrap();

        rebuild_indexes(&root).unwrap();

        let folder_index = fs::read_to_string(root.join("projects/index.toml")).unwrap();
        let value: toml::Value = toml::from_str(&folder_index).unwrap();
        assert_eq!(
            value.get("name").and_then(toml::Value::as_str),
            Some("Projects \"Quoted\"")
        );
        assert_eq!(
            value.get("description").and_then(toml::Value::as_str),
            Some("Line one\nLine two")
        );

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

    #[test]
    fn rebuild_ignores_vendor_directories() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/opex/node_modules/pkg")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            "type = \"project\"\nname = \"Projects\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/opex/index.md"), "# Opex\n").unwrap();
        fs::write(
            root.join("projects/opex/index.toml"),
            "type = \"project\"\nname = \"Opex\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        // A vendored folder that itself has an index pair; without pruning it
        // would be recorded as a subfolder of opex and folded into its checksum.
        fs::write(
            root.join("projects/opex/node_modules/pkg/index.md"),
            "# pkg\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/opex/node_modules/pkg/index.toml"),
            "type = \"project\"\nname = \"pkg\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();

        rebuild_indexes(&root).unwrap();

        let opex_index = fs::read_to_string(root.join("projects/opex/index.toml")).unwrap();
        assert!(
            !opex_index.contains("node_modules"),
            "vendored dir leaked into index: {opex_index}"
        );
        assert!(!root.join("projects/opex/node_modules/index.toml").exists());

        // Churn inside the ignored tree must not change the folder checksum.
        fs::write(
            root.join("projects/opex/node_modules/pkg/extra.md"),
            "# extra\n",
        )
        .unwrap();
        rebuild_indexes(&root).unwrap();
        let opex_index_again = fs::read_to_string(root.join("projects/opex/index.toml")).unwrap();
        assert_eq!(
            opex_index, opex_index_again,
            "ignored churn changed the folder index/checksum"
        );

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
