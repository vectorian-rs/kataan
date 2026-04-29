use std::{
    fmt::Display,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use kataan_core::{vault::LoadedVault, Error};

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault_path: Arc<PathBuf>,
    pub vault: Arc<RwLock<Option<LoadedVault>>>,
    pub boot_error: Arc<RwLock<Option<String>>>,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> Self {
        let (loaded, boot_error) = match LoadedVault::load(&vault_path) {
            Ok(vault) => (Some(vault), None),
            Err(error) => (None, Some(format_boot_error(&error))),
        };

        Self {
            vault_path: Arc::new(vault_path),
            vault: Arc::new(RwLock::new(loaded)),
            boot_error: Arc::new(RwLock::new(boot_error)),
        }
    }

    pub fn reload(&self) -> kataan_core::Result<()> {
        let loaded = LoadedVault::load(self.vault_path.as_ref())?;
        let mut vault = self.vault.write().map_err(|_| {
            kataan_core::Error::InvalidVaultStructure("vault lock poisoned".to_owned())
        })?;
        *vault = Some(loaded);
        let mut boot_error = self.boot_error.write().map_err(|_| {
            kataan_core::Error::InvalidVaultStructure("boot error lock poisoned".to_owned())
        })?;
        *boot_error = None;
        Ok(())
    }

    pub fn boot_error(&self) -> Option<String> {
        self.boot_error.read().ok().and_then(|error| error.clone())
    }
}

fn format_boot_error(error: &Error) -> String {
    match error {
        Error::TomlParse { path, source } => format!(
            "Invalid TOML metadata at {}: {}. The server is running in degraded mode; fix this file or run validation for the full diagnostic list.",
            path.display(),
            toml_error_message(source)
        ),
        Error::Io { path, source } => format!(
            "Could not read vault file at {}: {}. The server is running in degraded mode.",
            path.display(),
            source
        ),
        Error::InvalidCanonicalIdAtPath { path, source } => format!(
            "Invalid canonical ID at {}: {}. The server is running in degraded mode.",
            path.display(),
            source
        ),
        Error::InvalidCanonicalId(source) => format!(
            "Invalid canonical ID while loading vault: {source}. The server is running in degraded mode."
        ),
        Error::InvalidVaultStructure(message) => format!(
            "Invalid vault structure: {message}. The server is running in degraded mode."
        ),
    }
}

fn toml_error_message(source: &impl Display) -> String {
    source
        .to_string()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .next_back()
        .unwrap_or("invalid TOML")
        .trim()
        .to_owned()
}
