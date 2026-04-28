use std::path::{Path, PathBuf};

use crate::{
    document::DocumentMetadata,
    id::CanonicalId,
    index::{FolderIndex, VaultIndex},
    Error, Result,
};

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub index: VaultIndex,
}

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub id: CanonicalId,
    pub metadata: DocumentMetadata,
    pub markdown: String,
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index_path = root.join("index.toml");
        let index_text = std::fs::read_to_string(&index_path).map_err(|source| Error::Io {
            path: index_path.clone(),
            source,
        })?;
        let index = toml::from_str(&index_text).map_err(|source| Error::TomlParse {
            path: index_path,
            source,
        })?;
        Ok(Self { root, index })
    }

    pub fn load_folder_index(&self, folder: &str) -> Result<FolderIndex> {
        let path = self.root.join(folder).join("index.toml");
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| Error::TomlParse { path, source })
    }

    pub fn load_document(&self, id: &CanonicalId) -> Result<LoadedDocument> {
        let toml_path = self.root.join(id.toml_path());
        let toml_text = std::fs::read_to_string(&toml_path).map_err(|source| Error::Io {
            path: toml_path.clone(),
            source,
        })?;
        let metadata: DocumentMetadata =
            toml::from_str(&toml_text).map_err(|source| Error::TomlParse {
                path: toml_path,
                source,
            })?;

        let markdown_path = self.root.join(id.folder()).join(&metadata.markdown);
        let markdown = std::fs::read_to_string(&markdown_path).map_err(|source| Error::Io {
            path: markdown_path,
            source,
        })?;

        Ok(LoadedDocument {
            id: id.clone(),
            metadata,
            markdown,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn loads_folder_index() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/index.toml"),
            r#"name = "Projects"
description = "Project docs"
default_type = "project"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let index = vault.load_folder_index("projects").unwrap();

        assert_eq!(index.name, "Projects");
        assert_eq!(index.default_type.as_deref(), Some("project"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_document_metadata_and_markdown() {
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
status = "active"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        let document = vault.load_document(&id).unwrap();

        assert_eq!(document.id, id);
        assert_eq!(document.metadata.r#type, "project");
        assert_eq!(document.markdown, "# Kataan Redesign\n");

        fs::remove_dir_all(root).unwrap();
    }

    fn write_root_index(root: &Path) {
        fs::write(
            root.join("index.toml"),
            r#"schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
project = "projects"
"#,
        )
        .unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kataan-vault-test-{}-{counter}",
            std::process::id()
        ))
    }
}
