use std::path::Path;

use crate::{Error, Result};

/// Computes a BLAKE3 checksum over exact raw file bytes.
pub fn blake3_file(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}
