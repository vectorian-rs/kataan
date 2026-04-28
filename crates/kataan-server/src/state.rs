use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault_path: Arc<PathBuf>,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> Self {
        Self {
            vault_path: Arc::new(vault_path),
        }
    }
}
