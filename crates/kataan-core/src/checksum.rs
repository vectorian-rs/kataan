use std::path::Path;

use crate::{index::FolderDocument, Error, Result};

/// Computes a BLAKE3 checksum over exact raw bytes.
pub fn blake3_bytes(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// Computes a BLAKE3 checksum over exact raw file bytes.
pub fn blake3_file(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(blake3_bytes(&bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubfolderChecksum {
    pub name: String,
    pub folder_checksum: String,
}

/// Computes a deterministic recursive Merkle-style folder checksum.
///
/// Entries are sorted by kind/name before hashing. Documents include Markdown and
/// TOML checksums. Subfolders contribute their already-computed folder checksum.
pub fn folder_checksum_recursive(
    documents: &[FolderDocument],
    subfolders: &[SubfolderChecksum],
) -> String {
    let mut entries = Vec::new();

    for document in documents {
        entries.push(format!(
            "doc:{}:md:{}",
            document.slug, document.markdown_checksum
        ));
        entries.push(format!(
            "doc:{}:toml:{}",
            document.slug, document.toml_checksum
        ));
    }

    for subfolder in subfolders {
        entries.push(format!(
            "subfolder:{}:{}",
            subfolder.name, subfolder.folder_checksum
        ));
    }

    entries.sort();
    let mut input = entries.join("\n");
    if !input.is_empty() {
        input.push('\n');
    }
    blake3_bytes(input.as_bytes())
}

/// Computes a folder checksum for direct documents only.
///
/// New recursive callers should prefer `folder_checksum_recursive` and pass
/// subfolder checksums.
pub fn folder_checksum(documents: &[FolderDocument]) -> String {
    folder_checksum_recursive(documents, &[])
}

/// Computes a recursive folder checksum directly from files on disk.
///
/// The folder's own `index.md` and `index.toml` are included as `doc:index:*`
/// entries in that folder's checksum. Direct subfolders are included by name
/// using their recursively computed checksum.
pub fn folder_checksum_from_files(folder_path: impl AsRef<Path>) -> Result<String> {
    let folder_path = folder_path.as_ref();
    let mut documents = Vec::new();
    let mut subfolders = Vec::new();

    let index_md = folder_path.join("index.md");
    let index_toml = folder_path.join("index.toml");
    if index_md.exists() && index_toml.exists() {
        documents.push(FolderDocument {
            slug: "index".to_owned(),
            markdown: "index.md".to_owned(),
            toml: "index.toml".to_owned(),
            markdown_checksum: blake3_file(&index_md)?,
            toml_checksum: blake3_file(&index_toml)?,
        });
    }

    for entry in std::fs::read_dir(folder_path).map_err(|source| Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if path.join("index.toml").exists() && path.join("index.md").exists() {
                subfolders.push(SubfolderChecksum {
                    name,
                    folder_checksum: folder_checksum_from_files(&path)?,
                });
            }
            continue;
        }

        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || name == "index.md"
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

        documents.push(FolderDocument {
            slug: stem.to_owned(),
            markdown: name,
            toml: format!("{stem}.toml"),
            markdown_checksum: blake3_file(&path)?,
            toml_checksum: blake3_file(&toml_path)?,
        });
    }

    Ok(folder_checksum_recursive(&documents, &subfolders))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn blake3_file_hashes_exact_raw_bytes() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("note.md");
        fs::write(&path, b"hello\n").unwrap();

        assert_eq!(blake3_file(&path).unwrap(), blake3_bytes(b"hello\n"));
        assert_ne!(blake3_file(&path).unwrap(), blake3_bytes(b"hello\r\n"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_checksum_is_sorted_by_slug() {
        let first = folder_checksum(&[
            document("b", "md-b", "toml-b"),
            document("a", "md-a", "toml-a"),
        ]);
        let second = folder_checksum(&[
            document("a", "md-a", "toml-a"),
            document("b", "md-b", "toml-b"),
        ]);

        assert_eq!(first, second);
    }

    #[test]
    fn recursive_folder_checksum_is_sorted_by_subfolder_name() {
        let first = folder_checksum_recursive(
            &[document("a", "md-a", "toml-a")],
            &[subfolder("z", "checksum-z"), subfolder("b", "checksum-b")],
        );
        let second = folder_checksum_recursive(
            &[document("a", "md-a", "toml-a")],
            &[subfolder("b", "checksum-b"), subfolder("z", "checksum-z")],
        );

        assert_eq!(first, second);
    }

    #[test]
    fn folder_checksum_changes_when_markdown_toml_or_subfolder_changes() {
        let baseline = folder_checksum_recursive(
            &[document("a", "md-a", "toml-a")],
            &[subfolder("child", "folder-a")],
        );
        let markdown_changed = folder_checksum_recursive(
            &[document("a", "md-b", "toml-a")],
            &[subfolder("child", "folder-a")],
        );
        let toml_changed = folder_checksum_recursive(
            &[document("a", "md-a", "toml-b")],
            &[subfolder("child", "folder-a")],
        );
        let subfolder_changed = folder_checksum_recursive(
            &[document("a", "md-a", "toml-a")],
            &[subfolder("child", "folder-b")],
        );

        assert_ne!(baseline, markdown_changed);
        assert_ne!(baseline, toml_changed);
        assert_ne!(baseline, subfolder_changed);
    }

    #[test]
    fn folder_checksum_from_files_includes_index_and_subfolder() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("child")).unwrap();
        fs::write(root.join("index.md"), "# Root\n").unwrap();
        fs::write(root.join("index.toml"), "name = \"Root\"\n").unwrap();
        fs::write(root.join("note.md"), "# Note\n").unwrap();
        fs::write(root.join("note.toml"), "markdown = \"note.md\"\n").unwrap();
        fs::write(root.join("child/index.md"), "# Child\n").unwrap();
        fs::write(root.join("child/index.toml"), "name = \"Child\"\n").unwrap();

        let baseline = folder_checksum_from_files(&root).unwrap();
        fs::write(root.join("child/index.md"), "# Child changed\n").unwrap();
        let changed = folder_checksum_from_files(&root).unwrap();

        assert_ne!(baseline, changed);

        fs::remove_dir_all(root).unwrap();
    }

    fn document(slug: &str, markdown_checksum: &str, toml_checksum: &str) -> FolderDocument {
        FolderDocument {
            slug: slug.to_owned(),
            markdown: format!("{slug}.md"),
            toml: format!("{slug}.toml"),
            markdown_checksum: markdown_checksum.to_owned(),
            toml_checksum: toml_checksum.to_owned(),
        }
    }

    fn subfolder(name: &str, folder_checksum: &str) -> SubfolderChecksum {
        SubfolderChecksum {
            name: name.to_owned(),
            folder_checksum: folder_checksum.to_owned(),
        }
    }

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kataan-checksum-test-{}-{counter}",
            std::process::id()
        ))
    }
}
