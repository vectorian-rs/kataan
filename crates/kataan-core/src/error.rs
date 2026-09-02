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

    #[error("invalid canonical ID at {path}: {source}")]
    InvalidCanonicalIdAtPath {
        path: PathBuf,
        source: CanonicalIdError,
    },

    #[error("invalid vault structure: {0}")]
    InvalidVaultStructure(String),

    /// The vault was written by a newer kataan than this build. Distinct from
    /// `TomlParse` on purpose: the vault is not malformed, this binary is old.
    #[error(
        "vault at {path} declares schema {found}, but this build supports up to {supported}. \
         Reinstall kataan from a revision that supports it, for example \
         `cargo install --path crates/kataan-cli --root ~ --force`"
    )]
    UnsupportedSchemaVersion {
        path: PathBuf,
        found: String,
        supported: String,
    },

    /// A caller asked for something the vault won't allow (unknown type,
    /// id collision, illegal edge, …) — a bad request, not on-disk corruption.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}
