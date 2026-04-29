use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use kataan_core::vault::LoadedVault;

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
            Err(error) => (None, Some(error.to_string())),
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
