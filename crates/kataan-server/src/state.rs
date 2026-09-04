use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use kataan_core::vault::LoadedVault;
use kataan_search::SearchIndex;

use crate::{
    ignore::VaultIgnore,
    watch::{SharedWatchStatus, WatchStatus},
};

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault_path: Arc<PathBuf>,
    // The loaded vault is shared behind an `Arc` so read handlers clone a
    // refcount instead of deep-copying every document, graph edge, and type
    // definition on each request. `reload` swaps the inner `Arc` atomically.
    pub vault: Arc<RwLock<Arc<LoadedVault>>>,
    // The `.gitignore` matcher is compiled once and shared, so file/folder
    // handlers no longer re-read `.gitignore` and recompile the globs on every
    // request. `reload` rebuilds it when the vault (or its ignore rules) change.
    ignore: Arc<RwLock<Arc<VaultIgnore>>>,
    // Cached search index handle (path only — the SQLite file is created lazily
    // on first use), so `/search` and `/search/reindex` don't re-resolve and
    // re-open it per request.
    pub search: Arc<SearchIndex>,
    pub watch: SharedWatchStatus,
    /// Serializes vault mutations: one writer at a time.
    ///
    /// `kataan_core::mutate` is read-modify-write on files, then a full index
    /// rebuild. Two HTTP writes racing could interleave a read of the old
    /// sidecar with a write of the new one, and their rebuilds could cross.
    /// MCP never needed this — it is one process over stdio, so its writes were
    /// already serial — but the HTTP surface is concurrent by construction.
    ///
    /// Held only inside the blocking closure that performs the write, so
    /// waiting writers park on the blocking pool rather than an async worker.
    pub writes: Arc<std::sync::Mutex<()>>,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> kataan_core::Result<Self> {
        let loaded = LoadedVault::load(&vault_path)?;
        let ignore = load_ignore(&vault_path);
        let search = SearchIndex::at_default_path(&vault_path);
        Ok(Self {
            vault_path: Arc::new(vault_path),
            vault: Arc::new(RwLock::new(Arc::new(loaded))),
            ignore: Arc::new(RwLock::new(ignore)),
            search: Arc::new(search),
            watch: Arc::new(RwLock::new(WatchStatus::default())),
            writes: Arc::new(std::sync::Mutex::new(())),
        })
    }

    pub fn reload(&self) -> kataan_core::Result<()> {
        let loaded = LoadedVault::load(self.vault_path.as_ref())?;
        {
            let mut vault = self.vault.write().map_err(|_| {
                kataan_core::Error::InvalidVaultStructure("vault lock poisoned".to_owned())
            })?;
            *vault = Arc::new(loaded);
        }
        let ignore = load_ignore(self.vault_path.as_ref());
        if let Ok(mut guard) = self.ignore.write() {
            *guard = ignore;
        }
        Ok(())
    }

    /// The shared, pre-compiled `.gitignore` matcher for the vault.
    pub fn ignore(&self) -> Arc<VaultIgnore> {
        // On poison, recover the last-good matcher rather than failing open to
        // built-in-only ignores (which would silently stop honoring `.gitignore`).
        let guard = self
            .ignore
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        Arc::clone(&guard)
    }
}

fn load_ignore(vault_path: &Path) -> Arc<VaultIgnore> {
    match VaultIgnore::load(vault_path) {
        Ok(ignore) => Arc::new(ignore),
        Err(error) => {
            tracing::warn!(error = %error, "failed to compile vault ignore rules; using built-in ignores only");
            Arc::new(VaultIgnore::empty(vault_path))
        }
    }
}
