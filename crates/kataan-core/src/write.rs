use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

pub fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tempfile = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    tempfile.write_all(bytes).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    tempfile.as_file().sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    tempfile.persist(path).map_err(|error| Error::Io {
        path: PathBuf::from(path),
        source: error.error,
    })?;

    if let Ok(parent_dir) = std::fs::File::open(parent) {
        let _ = parent_dir.sync_all();
    }

    Ok(())
}

pub fn atomic_write_string(path: impl AsRef<Path>, content: &str) -> Result<()> {
    atomic_write(path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn writes_file_atomically() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("file.txt");

        atomic_write_string(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        atomic_write_string(&path, "goodbye").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "goodbye");

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("write")
    }
}
