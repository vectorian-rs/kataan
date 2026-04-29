use std::path::{Path, PathBuf};

use crate::{id::CanonicalId, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultEntry {
    FolderIndex {
        id: CanonicalId,
        markdown_path: PathBuf,
        toml_path: PathBuf,
    },
    Document {
        id: CanonicalId,
        markdown_path: PathBuf,
        toml_path: PathBuf,
    },
}

pub fn walk_type_folder(root: &Path, type_folder: &str) -> Result<Vec<VaultEntry>> {
    let mut entries = Vec::new();
    let relative_folder = Path::new(type_folder);
    if root.join(relative_folder).exists() {
        walk_folder(root, relative_folder, &mut entries)?;
    }
    entries.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(entries)
}

fn walk_folder(root: &Path, relative_folder: &Path, entries: &mut Vec<VaultEntry>) -> Result<()> {
    let folder_path = root.join(relative_folder);
    let index_md = folder_path.join("index.md");
    let index_toml = folder_path.join("index.toml");
    if index_md.exists() && index_toml.exists() {
        let id = CanonicalId::from_document_path(relative_folder.join("index.toml"))
            .map_err(|_| Error::ValidationFailed)?;
        entries.push(VaultEntry::FolderIndex {
            id,
            markdown_path: index_md,
            toml_path: index_toml,
        });
    }

    for entry in std::fs::read_dir(&folder_path).map_err(|source| Error::Io {
        path: folder_path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: folder_path.clone(),
            source,
        })?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            walk_folder(root, &relative_folder.join(file_name), entries)?;
            continue;
        }

        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || file_name == "index.md"
        {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{stem}.toml"));
        if !toml_path.exists() {
            continue;
        }

        let id = CanonicalId::from_document_path(relative_folder.join(&file_name))
            .map_err(|_| Error::ValidationFailed)?;
        entries.push(VaultEntry::Document {
            id,
            markdown_path: path,
            toml_path,
        });
    }

    Ok(())
}

impl VaultEntry {
    pub fn id(&self) -> &CanonicalId {
        match self {
            VaultEntry::FolderIndex { id, .. } | VaultEntry::Document { id, .. } => id,
        }
    }

    pub fn markdown_path(&self) -> &Path {
        match self {
            VaultEntry::FolderIndex { markdown_path, .. }
            | VaultEntry::Document { markdown_path, .. } => markdown_path,
        }
    }

    pub fn toml_path(&self) -> &Path {
        match self {
            VaultEntry::FolderIndex { toml_path, .. } | VaultEntry::Document { toml_path, .. } => {
                toml_path
            }
        }
    }

    pub fn is_folder_index(&self) -> bool {
        matches!(self, VaultEntry::FolderIndex { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn walks_folder_indexes_and_documents_recursively() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x")).unwrap();
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "type = \"project\"\n").unwrap();
        fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
        fs::write(
            root.join("projects/company-x/index.toml"),
            "type = \"project\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/company-x/q2.md"), "# Q2\n").unwrap();
        fs::write(
            root.join("projects/company-x/q2.toml"),
            "type = \"project\"\n",
        )
        .unwrap();

        let entries = walk_type_folder(&root, "projects").unwrap();
        let ids = entries
            .iter()
            .map(|entry| entry.id().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["projects", "projects/company-x", "projects/company-x/q2"]
        );
        assert!(entries[0].is_folder_index());
        assert!(!entries[2].is_folder_index());

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("kataan-walk-test-{}-{counter}", std::process::id()))
    }
}
