use std::path::PathBuf;

use crate::id::CanonicalIdError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("TOML parse error at {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("invalid canonical ID: {0}")]
    InvalidCanonicalId(#[from] CanonicalIdError),

    #[error("invalid vault structure: {0}")]
    InvalidVaultStructure(String),
}
