use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    checksum,
    constants::{
        CODE_FOLDER, DEFAULT_MAX_FOLDER_DEPTH, SCHEMA_VERSION, TYPE_CODE, VAULT_CONFIG_FILE,
    },
    rebuild::rebuild_indexes,
    title::title_case_slug,
    write, Error, Result,
};
const DEFAULT_ONTOLOGY: &str = include_str!("../templates/default-ontology.toml");

#[derive(Serialize)]
struct VaultToml<'a> {
    schema_version: &'a str,
    name: &'a str,
    created_at: &'a str,
    updated_at: &'a str,
    limits: VaultLimitsToml,
    type_folders: BTreeMap<&'a str, &'a str>,
}

#[derive(Serialize)]
struct VaultLimitsToml {
    max_folder_depth: usize,
}

#[derive(Serialize)]
struct InitialFolderIndexToml<'a> {
    #[serde(rename = "type")]
    document_type: &'a str,
    name: &'a str,
    description: &'a str,
    default_type: &'a str,
    markdown: &'a str,
    folder_checksum: &'a str,
}

pub fn init_vault(root: impl AsRef<Path>, name: &str) -> Result<()> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;

    let now = "2026-04-28T12:00:00Z";
    let config = VaultToml {
        schema_version: SCHEMA_VERSION,
        name,
        created_at: now,
        updated_at: now,
        limits: VaultLimitsToml {
            max_folder_depth: DEFAULT_MAX_FOLDER_DEPTH,
        },
        type_folders: BTreeMap::from([
            ("intake", "intake"),
            ("project", "projects"),
            ("person", "people"),
            ("note", "notes"),
            ("topic", "topics"),
            ("code", "code"),
            ("type-definition", "type"),
        ]),
    };
    let config_text = toml::to_string_pretty(&config).expect("serialize vault config TOML");
    write_file(&root.join(VAULT_CONFIG_FILE), &config_text)?;

    write_file(&root.join("ontology.toml"), DEFAULT_ONTOLOGY)?;

    for (folder, title, description, default_type) in [
        (
            "intake",
            "Intake",
            "Original source material before transformation.",
            "intake",
        ),
        (
            "projects",
            "Projects",
            "Active and historical efforts with goals, owners, and outcomes.",
            "project",
        ),
        ("people", "People", "People profiles.", "person"),
        ("notes", "Notes", "Curated notes.", "note"),
        ("topics", "Topics", "Durable concepts and themes.", "topic"),
        ("type", "Types", "Type definitions.", "type-definition"),
    ] {
        let folder_path = root.join(folder);
        std::fs::create_dir_all(&folder_path).map_err(|source| Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        write_file(
            &folder_path.join("index.md"),
            &format!("# {title}\n\n{description}\n"),
        )?;
        let folder_index = InitialFolderIndexToml {
            document_type: default_type,
            name: title,
            description,
            default_type,
            markdown: "index.md",
            folder_checksum: "blake3:todo",
        };
        let folder_index_text =
            toml::to_string_pretty(&folder_index).expect("serialize folder index TOML");
        write_file(&folder_path.join("index.toml"), &folder_index_text)?;
    }

    let code_path = root.join(CODE_FOLDER);
    std::fs::create_dir_all(&code_path).map_err(|source| Error::Io {
        path: code_path.clone(),
        source,
    })?;

    for ty in [
        "intake",
        "project",
        "person",
        "note",
        "topic",
        TYPE_CODE,
        "type-definition",
    ] {
        let title = title_case_slug(ty);
        let md_path = root.join("type").join(format!("{ty}.md"));
        write_file(
            &md_path,
            &format!("# {title}\n\nType definition for `{ty}`.\n"),
        )?;
        let markdown_checksum = checksum::blake3_file(&md_path)?;
        write_file(
            &root.join("type").join(format!("{ty}.toml")),
            &format!(
                r#"type = "type-definition"
name = "{ty}"
folder = "{}"
icon = "{}"

markdown = "{ty}.md"
markdown_checksum = "{markdown_checksum}"

created_by = "system"
last_updated_by = "system"
"#,
                type_folder(ty),
                type_icon(ty)
            ),
        )?;
    }

    rebuild_indexes(root)?;

    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    write::atomic_write_string(path, content)
}

fn type_folder(ty: &str) -> &str {
    match ty {
        "intake" => "intake",
        "project" => "projects",
        "person" => "people",
        "note" => "notes",
        "topic" => "topics",
        TYPE_CODE => CODE_FOLDER,
        "type-definition" => "type",
        _ => ty,
    }
}

fn type_icon(ty: &str) -> &str {
    match ty {
        "intake" => "Inbox",
        "project" => "FolderKanban",
        "person" => "User",
        "note" => "FileText",
        "topic" => "Lightbulb",
        TYPE_CODE => "Code",
        "type-definition" => "Boxes",
        _ => "Circle",
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn initializes_minimal_vault_structure() {
        let root = unique_temp_dir();

        init_vault(&root, "My Knowledgebase").unwrap();

        assert!(root.join("kataan.toml").exists());
        for folder in ["intake", "projects", "people", "notes", "topics", "type"] {
            assert!(
                root.join(folder).join("index.toml").exists(),
                "missing {folder}/index.toml"
            );
        }
        assert!(root.join("code").is_dir());
        assert!(!root.join("code/index.toml").exists());

        for ty in [
            "intake",
            "project",
            "person",
            "note",
            "topic",
            "code",
            "type-definition",
        ] {
            assert!(root.join("type").join(format!("{ty}.md")).exists());
            assert!(root.join("type").join(format!("{ty}.toml")).exists());
        }

        fs::write(root.join("code/tool.py"), "print('hello')\n").unwrap();

        let root_index = fs::read_to_string(root.join("kataan.toml")).unwrap();
        assert!(root_index.contains("schema_version = \"0.1.0\""));
        assert!(root_index.contains("name = \"My Knowledgebase\""));

        for folder in ["intake", "projects", "people", "notes", "topics", "type"] {
            let folder_index = fs::read_to_string(root.join(folder).join("index.toml")).unwrap();
            assert!(!folder_index.contains("blake3:todo"));
        }

        let report = crate::validate::validate(&root).unwrap();
        assert!(report.is_ok(), "{:#?}", report.diagnostics);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn escapes_vault_name_in_toml() {
        let root = unique_temp_dir();
        let name = "My \"Quoted\"\nVault";

        init_vault(&root, name).unwrap();

        let vault = crate::vault::Vault::open(&root).unwrap();
        assert_eq!(vault.index.name, name);

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("init")
    }
}
