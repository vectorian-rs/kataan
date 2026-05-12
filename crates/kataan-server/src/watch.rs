use std::{
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, RwLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tracing::{debug, error, info};

use crate::state::AppState;

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
    pub diagnostics: Vec<WatchDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
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
    let initial_fingerprint = vault_fingerprint(&vault_path)?;
    set_status(&state.watch, |status| {
        status.last_fingerprint = Some(initial_fingerprint);
    });

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result| {
        if tx.send(result).is_err() {
            debug!("filesystem watcher event receiver dropped");
        }
    })?;
    watcher.watch(&vault_path, RecursiveMode::Recursive)?;
    info!(vault = %vault_path.display(), "filesystem watcher started");

    let mut pending = false;
    let mut last_event = SystemTime::now();

    loop {
        match rx.recv_timeout(WATCH_CHANNEL_TIMEOUT) {
            Ok(Ok(event)) => {
                if event
                    .paths
                    .iter()
                    .all(|path| should_ignore_path(&vault_path, path))
                {
                    continue;
                }
                pending = true;
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
                if pending && last_event.elapsed().unwrap_or_default() >= DEBOUNCE {
                    pending = false;
                    process_change(&state);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("filesystem watcher event channel disconnected");
            }
        }
    }
}

fn process_change(state: &AppState) {
    match process_change_inner(state) {
        Ok(()) => {}
        Err(error) => {
            error!(error = %error, "failed to process filesystem change");
            set_status(&state.watch, |status| {
                status.last_processed_at = Some(timestamp(SystemTime::now()));
                status.last_error = Some(error.to_string());
                status.revision += 1;
            });
        }
    }
}

fn process_change_inner(state: &AppState) -> anyhow::Result<()> {
    let fingerprint = vault_fingerprint(state.vault_path.as_ref())?;
    if current_fingerprint(&state.watch).as_deref() == Some(fingerprint.as_str()) {
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

    let mut final_fingerprint = fingerprint;
    let mut rebuilt = false;
    if should_rebuild {
        info!(vault = %state.vault_path.display(), "filesystem watcher rebuilding repairable index drift");
        kataan_core::rebuild::rebuild_indexes(state.vault_path.as_ref())?;
        rebuilt = true;
        final_fingerprint = vault_fingerprint(state.vault_path.as_ref())?;
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

fn watch_diagnostics(report: &kataan_core::diagnostic::DiagnosticReport) -> Vec<WatchDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| WatchDiagnostic {
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            path: diagnostic.path.clone(),
        })
        .collect()
}

fn is_rebuild_repairable(code: &str) -> bool {
    matches!(code, "checksum-mismatch" | "index-drift")
}

fn vault_fingerprint(root: &Path) -> anyhow::Result<String> {
    let mut entries = Vec::new();
    collect_fingerprint_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut input = String::new();
    for (relative, hash) in entries {
        input.push_str(&relative);
        input.push('\0');
        input.push_str(&hash);
        input.push('\n');
    }
    Ok(kataan_core::checksum::blake3_bytes(input.as_bytes()))
}

fn collect_fingerprint_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if should_ignore_path(root, &path) {
            continue;
        }
        if path.is_dir() {
            collect_fingerprint_entries(root, &path, entries)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            let hash = kataan_core::checksum::blake3_file(&path)?;
            entries.push((relative, hash));
        }
    }
    Ok(())
}

fn should_ignore_path(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | "dist" | ".astro"
        ) || name.starts_with(".swp")
            || name.ends_with('~')
    })
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
}
