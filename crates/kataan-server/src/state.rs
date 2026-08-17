use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use kataan_core::vault::LoadedVault;

use crate::watch::{SharedWatchStatus, WatchStatus};

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault_path: Arc<PathBuf>,
    // The loaded vault is shared behind an `Arc` so read handlers clone a
    // refcount instead of deep-copying every document, graph edge, and type
    // definition on each request. `reload` swaps the inner `Arc` atomically.
    pub vault: Arc<RwLock<Arc<LoadedVault>>>,
    pub watch: SharedWatchStatus,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> kataan_core::Result<Self> {
        let loaded = LoadedVault::load(&vault_path)?;
        Ok(Self {
            vault_path: Arc::new(vault_path),
            vault: Arc::new(RwLock::new(Arc::new(loaded))),
            watch: Arc::new(RwLock::new(WatchStatus::default())),
        })
    }

    pub fn reload(&self) -> kataan_core::Result<()> {
        let loaded = LoadedVault::load(self.vault_path.as_ref())?;
        let mut vault = self.vault.write().map_err(|_| {
            kataan_core::Error::InvalidVaultStructure("vault lock poisoned".to_owned())
        })?;
        *vault = Arc::new(loaded);
        Ok(())
    }
}
