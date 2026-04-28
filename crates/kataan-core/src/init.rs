use std::path::Path;

use crate::{checksum, Error, Result};

const SCHEMA_VERSION: &str = "0.1.0";

pub fn init_vault(root: impl AsRef<Path>, name: &str) -> Result<()> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;

    let now = "2026-04-28T12:00:00Z";
    write_file(
        &root.join("index.toml"),
        &format!(
            r#"schema_version = "{SCHEMA_VERSION}"
name = "{name}"
created_at = "{now}"
updated_at = "{now}"

[type_folders]
raw = "raw"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#
        ),
    )?;

    for (folder, title, description, default_type) in [
        (
            "raw",
            "Raw",
            "Original source material before transformation.",
            "raw",
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
            &folder_path.join("index.toml"),
            &format!(
                r#"name = "{title}"
description = "{description}"
default_type = "{default_type}"
folder_checksum = "blake3:todo"
"#
            ),
        )?;
    }

    for ty in [
        "raw",
        "project",
        "person",
        "note",
        "topic",
        "type-definition",
    ] {
        let title = title_case(ty);
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
icon = "circle"

markdown = "{ty}.md"
markdown_checksum = "{markdown_checksum}"

created_by = "system"
last_updated_by = "system"
"#,
                type_folder(ty)
            ),
        )?;
    }

    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn type_folder(ty: &str) -> &str {
    match ty {
        "raw" => "raw",
        "project" => "projects",
        "person" => "people",
        "note" => "notes",
        "topic" => "topics",
        "type-definition" => "type",
        _ => ty,
    }
}

fn title_case(value: &str) -> String {
    value
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn initializes_minimal_vault_structure() {
        let root = unique_temp_dir();

        init_vault(&root, "My Knowledgebase").unwrap();

        assert!(root.join("index.toml").exists());
        for folder in ["raw", "projects", "people", "notes", "topics", "type"] {
            assert!(
                root.join(folder).join("index.toml").exists(),
                "missing {folder}/index.toml"
            );
        }
        for ty in [
            "raw",
            "project",
            "person",
            "note",
            "topic",
            "type-definition",
        ] {
            assert!(root.join("type").join(format!("{ty}.md")).exists());
            assert!(root.join("type").join(format!("{ty}.toml")).exists());
        }

        let root_index = fs::read_to_string(root.join("index.toml")).unwrap();
        assert!(root_index.contains("schema_version = \"0.1.0\""));
        assert!(root_index.contains("name = \"My Knowledgebase\""));

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("kataan-init-test-{}-{counter}", std::process::id()))
    }
}
