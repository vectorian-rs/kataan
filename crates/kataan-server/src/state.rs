use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use kataan_core::vault::LoadedVault;

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault_path: Arc<PathBuf>,
    pub vault: Arc<RwLock<LoadedVault>>,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> kataan_core::Result<Self> {
        let loaded = LoadedVault::load(&vault_path)?;
        Ok(Self {
            vault_path: Arc::new(vault_path),
            vault: Arc::new(RwLock::new(loaded)),
        })
    }

    pub fn reload(&self) -> kataan_core::Result<()> {
        let loaded = LoadedVault::load(self.vault_path.as_ref())?;
        let mut vault = self.vault.write().map_err(|_| {
            kataan_core::Error::InvalidVaultStructure("vault lock poisoned".to_owned())
        })?;
        *vault = loaded;
        Ok(())
    }
}
