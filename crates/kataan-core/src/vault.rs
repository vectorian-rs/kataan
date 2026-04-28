use std::path::{Path, PathBuf};

use crate::{index::VaultIndex, Error, Result};

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub index: VaultIndex,
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
}
