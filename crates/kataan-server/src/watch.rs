use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, RwLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tracing::{debug, error, info};

use crate::{ignore::VaultIgnore, state::AppState};

const DEBOUNCE: Duration = Duration::from_millis(900);
const WATCH_CHANNEL_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Default, Serialize)]
pub struct WatchStatus {
    pub enabled: bool,
    pub revision: u64,
    pub last_event_at: Option<String>,
    pub last_processed_at: Option<String>,
    pub last_reload_at: Option<String>,
    pub last_rebuild_at: Option<String>,
    pub last_fingerprint: Option<String>,
    pub last_error: Option<String>,
    pub diagnostics: Vec<crate::api::DiagnosticResponse>,
}

pub type SharedWatchStatus = Arc<RwLock<WatchStatus>>;

pub fn spawn_watcher(state: AppState) {
    set_status(&state.watch, |status| {
        status.enabled = true;
        status.last_error = None;
    });

    let status = state.watch.clone();
    let vault_path = state.vault_path.as_ref().clone();
    thread::spawn(move || {
        if let Err(error) = watch_loop(state, vault_path) {
            error!(error = %error, "filesystem watcher stopped");
            set_status(&status, |status| {
                status.enabled = false;
                status.last_error = Some(error.to_string());
                status.revision += 1;
            });
        }
    });
}

fn watch_loop(state: AppState, vault_path: PathBuf) -> anyhow::Result<()> {
    let mut fingerprint = VaultFingerprint::scan(&vault_path, &state.ignore())?;
    set_status(&state.watch, |status| {
        status.last_fingerprint = Some(fingerprint.digest());
    });

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result| {
        if tx.send(result).is_err() {
            debug!("filesystem watcher event receiver dropped");
        }
    })?;
    watcher.watch(&vault_path, RecursiveMode::Recursive)?;
    info!(vault = %vault_path.display(), "filesystem watcher started");

    let mut changed: BTreeSet<PathBuf> = BTreeSet::new();
    let mut last_event = SystemTime::now();

    loop {
        match rx.recv_timeout(WATCH_CHANNEL_TIMEOUT) {
            Ok(Ok(event)) => {
                let ignore = state.ignore();
                if event
                    .paths
                    .iter()
                    .all(|path| ignore.should_ignore_path(path))
                {
                    continue;
                }
                changed.extend(event.paths);
                last_event = SystemTime::now();
                set_status(&state.watch, |status| {
                    status.last_event_at = Some(timestamp(last_event));
                    status.last_error = None;
                });
            }
            Ok(Err(error)) => {
                error!(error = %error, "filesystem watcher event error");
                set_status(&state.watch, |status| {
                    status.last_error = Some(error.to_string());
                    status.revision += 1;
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !changed.is_empty() && last_event.elapsed().unwrap_or_default() >= DEBOUNCE {
                    let batch = std::mem::take(&mut changed);
                    let ignore = state.ignore();
                    // Ignore-rule edits change which files count, so a stale
                    // incremental map could miss them: rescan fully in that case.
                    let updated = if batch.iter().any(|path| affects_ignore_rules(path)) {
                        VaultFingerprint::scan(&vault_path, &ignore)
                            .map(|scanned| fingerprint = scanned)
                    } else {
                        fingerprint.apply(&vault_path, &ignore, &batch)
                    };
                    match updated {
                        Ok(()) => process_change(&state, &mut fingerprint),
                        // A transient IO error (e.g. a file removed mid-hash by an
                        // editor's atomic save) must not kill the watcher thread.
                        // Put the batch back rather than dropping it: `apply`
                        // fails partway, so the paths it never reached would
                        // otherwise be lost, and the server would keep serving
                        // pre-edit content until some unrelated file changed.
                        Err(error) => {
                            changed.extend(batch.iter().cloned());
                            error!(error = %error, "failed to update vault fingerprint; retrying on next change");
                            set_status(&state.watch, |status| {
                                status.last_error = Some(error.to_string());
                                status.revision += 1;
                            });
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("filesystem watcher event channel disconnected");
            }
        }
    }
}

fn process_change(state: &AppState, fingerprint: &mut VaultFingerprint) {
    if let Err(error) = process_change_inner(state, fingerprint) {
        error!(error = %error, "failed to process filesystem change");
        set_status(&state.watch, |status| {
            status.last_processed_at = Some(timestamp(SystemTime::now()));
            status.last_error = Some(error.to_string());
            status.revision += 1;
        });
    }
}

fn process_change_inner(
    state: &AppState,
    fingerprint: &mut VaultFingerprint,
) -> anyhow::Result<()> {
    let digest = fingerprint.digest();
    if current_fingerprint(&state.watch).as_deref() == Some(digest.as_str()) {
        debug!(vault = %state.vault_path.display(), "filesystem change ignored; fingerprint unchanged");
        return Ok(());
    }

    debug!(vault = %state.vault_path.display(), "processing filesystem change");
    let report = kataan_core::validate::validate(state.vault_path.as_ref())?;
    let mut diagnostics = watch_diagnostics(&report);

    let should_rebuild = !diagnostics.is_empty()
        && diagnostics
            .iter()
            .all(|diagnostic| is_rebuild_repairable(&diagnostic.code));

    let mut final_fingerprint = digest;
    let mut rebuilt = false;
    if should_rebuild {
        info!(vault = %state.vault_path.display(), "filesystem watcher rebuilding repairable index drift");
        kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref())?;
        rebuilt = true;
        // Rebuild rewrites index files we did not observe as events, so rescan
        // to fold those writes in; otherwise the next cycle sees them as a change.
        *fingerprint = VaultFingerprint::scan(state.vault_path.as_ref(), &state.ignore())?;
        final_fingerprint = fingerprint.digest();
        diagnostics =
            watch_diagnostics(&kataan_core::validate::validate(state.vault_path.as_ref())?);
    }

    state.reload()?;
    let now = timestamp(SystemTime::now());
    set_status(&state.watch, |status| {
        status.last_processed_at = Some(now.clone());
        status.last_reload_at = Some(now.clone());
        if rebuilt {
            status.last_rebuild_at = Some(now);
        }
        status.last_fingerprint = Some(final_fingerprint);
        status.last_error = None;
        status.diagnostics = diagnostics;
        status.revision += 1;
    });
    Ok(())
}

fn watch_diagnostics(
    report: &kataan_core::diagnostic::DiagnosticReport,
) -> Vec<crate::api::DiagnosticResponse> {
    report
        .diagnostics
        .iter()
        .map(crate::api::DiagnosticResponse::from)
        .collect()
}

fn is_rebuild_repairable(code: &str) -> bool {
    use kataan_core::diagnostic_codes::{CHECKSUM_MISMATCH, INDEX_DRIFT};
    matches!(code, CHECKSUM_MISMATCH | INDEX_DRIFT)
}

/// Content fingerprint of the watched vault, maintained incrementally. Maps each
/// non-ignored file's root-relative slug to its blake3 hash; the digest is a
/// stable hash of the whole map. A single edit re-hashes one file via
/// [`apply`](Self::apply) instead of re-walking and re-hashing the entire tree.
#[derive(Default)]
struct VaultFingerprint {
    hashes: BTreeMap<String, String>,
}

impl VaultFingerprint {
    /// Full walk of the vault; used at startup and after a rebuild (which writes
    /// index files we did not observe as events).
    fn scan(root: &Path, ignore: &VaultIgnore) -> anyhow::Result<Self> {
        let mut fingerprint = Self::default();
        fingerprint.scan_dir(root, root, ignore)?;
        Ok(fingerprint)
    }

    fn scan_dir(
        &mut self,
        root: &Path,
        directory: &Path,
        ignore: &VaultIgnore,
    ) -> anyhow::Result<()> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if ignore.should_ignore_path(&path) {
                continue;
            }
            // Symlink-aware, matching `kataan_core::walk`: `ln -s . mirror`
            // inside a vault would otherwise recurse until the stack overflows,
            // which aborts the process rather than just the watcher.
            if kataan_core::walk::is_regular_dir(&path) {
                self.scan_dir(root, &path, ignore)?;
            } else if kataan_core::walk::is_regular_file(&path) {
                self.hashes.insert(
                    kataan_core::walk::relative_slug(root, &path),
                    kataan_core::checksum::blake3_file(&path)?,
                );
            }
        }
        Ok(())
    }

    /// Update the map for the given changed paths: existing files are re-hashed,
    /// directories are reconciled (subtree dropped then rescanned), and removed
    /// paths — plus everything under a removed directory — are dropped.
    fn apply(
        &mut self,
        root: &Path,
        ignore: &VaultIgnore,
        changed: &BTreeSet<PathBuf>,
    ) -> anyhow::Result<()> {
        for path in changed {
            let slug = kataan_core::walk::relative_slug(root, path);
            if ignore.should_ignore_path(path) {
                self.remove_subtree(&slug);
            } else if kataan_core::walk::is_regular_dir(path) {
                // Reconcile the whole subtree: drop stale entries first, so a file
                // deleted inside a directory reported only at directory granularity
                // (e.g. coalesced FSEvents) is removed, not left as a ghost.
                self.remove_subtree(&slug);
                self.scan_dir(root, path, ignore)?;
            } else if kataan_core::walk::is_regular_file(path) {
                self.hashes
                    .insert(slug, kataan_core::checksum::blake3_file(path)?);
            } else {
                self.remove_subtree(&slug);
            }
        }
        Ok(())
    }

    fn remove_subtree(&mut self, slug: &str) {
        let prefix = format!("{slug}/");
        self.hashes
            .retain(|key, _| key != slug && !key.starts_with(&prefix));
    }

    fn digest(&self) -> String {
        // BTreeMap iterates in key order, so the digest is order-independent.
        let mut input = String::new();
        for (relative, hash) in &self.hashes {
            input.push_str(relative);
            input.push('\0');
            input.push_str(hash);
            input.push('\n');
        }
        kataan_core::checksum::blake3_bytes(input.as_bytes())
    }
}

/// Whether a changed path can alter which files are ignored, forcing a full
/// rescan instead of an incremental update.
fn affects_ignore_rules(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(".gitignore")
}

fn current_fingerprint(status: &SharedWatchStatus) -> Option<String> {
    status
        .read()
        .ok()
        .and_then(|status| status.last_fingerprint.clone())
}

fn set_status(status: &SharedWatchStatus, update: impl FnOnce(&mut WatchStatus)) {
    if let Ok(mut status) = status.write() {
        update(&mut status);
    }
}

fn timestamp(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}Z", duration.as_secs(), duration.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_repairable_codes_are_limited_to_drift() {
        assert!(is_rebuild_repairable("checksum-mismatch"));
        assert!(is_rebuild_repairable("index-drift"));
        assert!(!is_rebuild_repairable("invalid-toml"));
        assert!(!is_rebuild_repairable("missing-folder-index"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("kataan-server-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn digest(root: &Path) -> String {
        let ignore = VaultIgnore::load(root).unwrap();
        VaultFingerprint::scan(root, &ignore).unwrap().digest()
    }

    #[test]
    fn fingerprint_respects_gitignore() {
        let root = temp_dir("ignore");
        std::fs::create_dir_all(root.join("ignored")).unwrap();
        std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::write(root.join("kept.md"), "one").unwrap();
        std::fs::write(root.join("ignored/file.md"), "one").unwrap();

        let initial = digest(&root);
        std::fs::write(root.join("ignored/file.md"), "two").unwrap();
        assert_eq!(initial, digest(&root), "ignored files must not affect it");
        std::fs::write(root.join("kept.md"), "two").unwrap();
        assert_ne!(initial, digest(&root), "tracked edits must change it");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_apply_matches_full_scan() {
        let root = temp_dir("incremental");
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("keep.md"), "keep").unwrap();
        std::fs::write(root.join("edit.md"), "before").unwrap();
        std::fs::write(root.join("gone.md"), "gone").unwrap();
        std::fs::write(root.join("dir/inside.md"), "inside").unwrap();

        let ignore = VaultIgnore::load(&root).unwrap();
        let mut incremental = VaultFingerprint::scan(&root, &ignore).unwrap();

        // Edit a file, add one, delete a file, and delete a whole directory.
        std::fs::write(root.join("edit.md"), "after").unwrap();
        std::fs::write(root.join("added.md"), "added").unwrap();
        std::fs::remove_file(root.join("gone.md")).unwrap();
        std::fs::remove_dir_all(root.join("dir")).unwrap();

        let changed: BTreeSet<PathBuf> = [
            root.join("edit.md"),
            root.join("added.md"),
            root.join("gone.md"),
            root.join("dir"),
        ]
        .into_iter()
        .collect();
        incremental.apply(&root, &ignore, &changed).unwrap();

        let full = VaultFingerprint::scan(&root, &ignore).unwrap();
        assert_eq!(
            incremental.digest(),
            full.digest(),
            "incremental update must converge to a full rescan"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_reconciles_deletion_reported_at_directory_granularity() {
        let root = temp_dir("dir-coalesce");
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("dir/a.md"), "a").unwrap();
        std::fs::write(root.join("dir/b.md"), "b").unwrap();

        let ignore = VaultIgnore::load(&root).unwrap();
        let mut incremental = VaultFingerprint::scan(&root, &ignore).unwrap();

        // Delete a file inside `dir`, but report only the still-existing parent
        // directory as changed (as coalesced FSEvents can).
        std::fs::remove_file(root.join("dir/a.md")).unwrap();
        let changed: BTreeSet<PathBuf> = [root.join("dir")].into_iter().collect();
        incremental.apply(&root, &ignore, &changed).unwrap();

        let full = VaultFingerprint::scan(&root, &ignore).unwrap();
        assert_eq!(
            incremental.digest(),
            full.digest(),
            "a deletion inside a directory-granularity event must not leave a ghost"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
