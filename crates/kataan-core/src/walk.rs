use std::path::{Path, PathBuf};

use crate::{id::CanonicalId, scan::ScanIgnore, Error, Result};

/// A vault-root-relative path rendered with forward slashes, for stable
/// cross-platform keys and identifiers. Falls back to the full path when it is
/// not under `root`.
pub fn relative_slug(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// True only for a real directory. Uses `symlink_metadata`, so a symbolic link
/// reads as "not a directory" — vault walkers therefore never follow symlinks
/// (no escaping the vault, and no infinite recursion through a symlink cycle).
pub fn is_regular_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

/// True only for a real file (a symlink reads as "not a file").
pub fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

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

pub fn walk_type_folder(
    root: &Path,
    type_folder: &str,
    ignore: &ScanIgnore,
) -> Result<Vec<VaultEntry>> {
    let mut entries = Vec::new();
    let relative_folder = Path::new(type_folder);
    if is_regular_dir(&root.join(relative_folder)) {
        walk_folder(root, relative_folder, ignore, &mut entries, 0)?;
    }
    entries.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(entries)
}

fn walk_folder(
    root: &Path,
    relative_folder: &Path,
    ignore: &ScanIgnore,
    entries: &mut Vec<VaultEntry>,
    depth: usize,
) -> Result<()> {
    if depth > crate::constants::MAX_WALK_DEPTH {
        return Err(crate::Error::InvalidVaultStructure(format!(
            "`{}` nests deeper than {} directories",
            relative_folder.display(),
            crate::constants::MAX_WALK_DEPTH
        )));
    }
    let folder_path = root.join(relative_folder);
    let index_md = folder_path.join("index.md");
    let index_toml = folder_path.join("index.toml");
    if is_regular_file(&index_md) && is_regular_file(&index_toml) {
        let relative_index = relative_folder.join("index.toml");
        let id = canonical_id_from_path(&relative_index)?;
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

        if is_regular_dir(&path) {
            if ignore.is_ignored(&path, true) {
                continue;
            }
            walk_folder(
                root,
                &relative_folder.join(file_name),
                ignore,
                entries,
                depth + 1,
            )?;
            continue;
        }

        if !is_regular_file(&path)
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || file_name == "index.md"
        {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let toml_path = folder_path.join(format!("{stem}.toml"));
        if !is_regular_file(&toml_path) {
            continue;
        }

        let relative_document = relative_folder.join(&file_name);
        let id = canonical_id_from_path(&relative_document)?;
        entries.push(VaultEntry::Document {
            id,
            markdown_path: path,
            toml_path,
        });
    }

    Ok(())
}

fn canonical_id_from_path(path: &Path) -> Result<CanonicalId> {
    CanonicalId::from_document_path(path).map_err(|source| Error::InvalidCanonicalIdAtPath {
        path: path.to_path_buf(),
        source,
    })
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
    fn reports_invalid_nested_paths_with_path_context() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company_x")).unwrap();
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(root.join("projects/index.toml"), "type = \"project\"\n").unwrap();
        fs::write(root.join("projects/company_x/index.md"), "# Company\n").unwrap();
        fs::write(
            root.join("projects/company_x/index.toml"),
            "type = \"project\"\n",
        )
        .unwrap();

        let error = walk_type_folder(&root, "projects", &ScanIgnore::none(&root)).unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidCanonicalIdAtPath { path, .. }
                if path.to_string_lossy() == "projects/company_x/index.toml"
        ));

        fs::remove_dir_all(root).unwrap();
    }

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

        let entries = walk_type_folder(&root, "projects", &ScanIgnore::none(&root)).unwrap();
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
        crate::test_support::unique_temp_dir("walk")
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    /// The walkers recursed once per directory with no bound, so a
    /// pathologically nested tree aborted the process on stack overflow rather
    /// than returning an error. `limits.max_folder_depth` did not help: it is
    /// checked after the walk, by `validate`.
    #[test]
    fn a_tree_deeper_than_the_cap_is_an_error_not_a_crash() {
        let root = crate::test_support::unique_temp_dir("walk-depth");
        let mut deep = root.join("notes");
        for level in 0..(crate::constants::MAX_WALK_DEPTH + 5) {
            deep = deep.join(format!("l{level}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("index.md"), "# deep\n").unwrap();
        std::fs::write(
            deep.join("index.toml"),
            "type = \"note\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();

        let ignore = crate::scan::ScanIgnore::load(&root, &Default::default()).unwrap();
        let result = walk_type_folder(&root, "notes", &ignore);

        assert!(
            matches!(result, Err(crate::Error::InvalidVaultStructure(_))),
            "expected a structural error, got {result:?}"
        );

        // The same tree is still walkable up to the cap, so the bound only
        // rejects what is already unreasonable.
        let shallow = crate::test_support::unique_temp_dir("walk-shallow");
        let mut path = shallow.join("notes");
        for level in 0..4 {
            path = path.join(format!("l{level}"));
        }
        std::fs::create_dir_all(&path).unwrap();
        let ignore = crate::scan::ScanIgnore::load(&shallow, &Default::default()).unwrap();
        assert!(walk_type_folder(&shallow, "notes", &ignore).is_ok());

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(shallow).unwrap();
    }
}
