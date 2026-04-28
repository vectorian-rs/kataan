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

/// Computes a deterministic Merkle-like folder checksum from folder document entries.
///
/// Entries are sorted by slug before hashing. The checksum includes each document's
/// Markdown checksum and TOML sidecar checksum, but not the folder `index.toml` file.
pub fn folder_checksum(documents: &[FolderDocument]) -> String {
    let mut documents = documents.to_vec();
    documents.sort_by(|left, right| left.slug.cmp(&right.slug));

    let mut input = String::new();
    for document in documents {
        input.push_str(&document.slug);
        input.push_str(":md:");
        input.push_str(&document.markdown_checksum);
        input.push('\n');
        input.push_str(&document.slug);
        input.push_str(":toml:");
        input.push_str(&document.toml_checksum);
        input.push('\n');
    }

    blake3_bytes(input.as_bytes())
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
    fn folder_checksum_changes_when_markdown_or_toml_checksum_changes() {
        let baseline = folder_checksum(&[document("a", "md-a", "toml-a")]);
        let markdown_changed = folder_checksum(&[document("a", "md-b", "toml-a")]);
        let toml_changed = folder_checksum(&[document("a", "md-a", "toml-b")]);

        assert_ne!(baseline, markdown_changed);
        assert_ne!(baseline, toml_changed);
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

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kataan-checksum-test-{}-{counter}",
            std::process::id()
        ))
    }
}
