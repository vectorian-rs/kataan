use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

pub fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    // A rename replaces the destination *inode*, so whatever mode the old file
    // had is gone with it. `NamedTempFile` creates at 0600, which silently
    // turned every rewritten sidecar private — a vault served from a group- or
    // world-readable directory lost that access on the next rebuild.
    let intended = intended_permissions(path);
    let mut tempfile = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    tempfile.write_all(bytes).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    tempfile.as_file().sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // Applied after creation, not at it: a file's creation mode is masked by
    // the process umask, so asking for 0664 under a 022 umask would quietly
    // yield 0644. `chmod` is not masked. Widening a file that started at 0600
    // is the safe direction to move in.
    if let Some(permissions) = intended {
        std::fs::set_permissions(tempfile.path(), permissions).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }

    tempfile.persist(path).map_err(|error| Error::Io {
        path: PathBuf::from(path),
        source: error.error,
    })?;

    if let Ok(parent_dir) = std::fs::File::open(parent) {
        let _ = parent_dir.sync_all();
    }

    Ok(())
}

pub fn atomic_write_string(path: impl AsRef<Path>, content: &str) -> Result<()> {
    atomic_write(path, content.as_bytes())
}

/// What the file at `path` should end up with: the mode it already has, or the
/// one an ordinary create would have produced.
///
/// Ownership is not preserved and cannot be: `rename` gives the new inode the
/// writing user, and restoring another user's ownership needs privileges the
/// writer does not generally have. Preserving the mode is the part that is both
/// achievable and load-bearing.
#[cfg(unix)]
fn intended_permissions(path: &Path) -> Option<std::fs::Permissions> {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions())
        .or_else(default_file_permissions)
}

#[cfg(not(unix))]
fn intended_permissions(path: &Path) -> Option<std::fs::Permissions> {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions())
}

/// The mode `File::create` would produce here — `0o666` masked by the process
/// umask.
///
/// Probed once rather than assumed: hard-coding 0644 would ignore a restrictive
/// umask, and leaving tempfile's 0600 would make every *new* document private
/// while existing ones kept their mode, which is worse than either.
#[cfg(unix)]
fn default_file_permissions() -> Option<std::fs::Permissions> {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::OnceLock;

    static MODE: OnceLock<Option<u32>> = OnceLock::new();
    let mode = *MODE.get_or_init(|| {
        let probe = std::env::temp_dir().join(format!(
            "kataan-umask-probe-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let file = std::fs::File::create(&probe).ok()?;
        let mode = file.metadata().ok().map(|m| m.permissions().mode());
        drop(file);
        let _ = std::fs::remove_file(&probe);
        mode
    });
    mode.map(std::fs::Permissions::from_mode)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    /// A rename replaces the inode, so the destination's mode goes with it
    /// unless it is carried over. Every rewritten sidecar silently became 0600,
    /// and a vault served from a group-readable directory lost that access on
    /// the next rebuild.
    #[cfg(unix)]
    #[test]
    fn an_existing_files_mode_survives_a_rewrite() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();

        for mode in [0o644, 0o600, 0o664] {
            let path = root.join(format!("file-{mode:o}.txt"));
            atomic_write_string(&path, "first").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();

            atomic_write_string(&path, "second").unwrap();

            let after = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(after, mode, "mode changed on rewrite");
            assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        }

        fs::remove_dir_all(root).unwrap();
    }

    /// A new file takes the mode an ordinary create would give it, rather than
    /// tempfile's private 0600 — otherwise new documents are unreadable to
    /// anyone else while existing ones keep their mode.
    #[cfg(unix)]
    #[test]
    fn a_new_file_takes_the_ordinary_create_mode() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();

        let reference = root.join("reference.txt");
        fs::File::create(&reference).unwrap();
        let expected = fs::metadata(&reference).unwrap().permissions().mode() & 0o777;

        let written = root.join("written.txt");
        atomic_write_string(&written, "x").unwrap();
        let actual = fs::metadata(&written).unwrap().permissions().mode() & 0o777;

        assert_eq!(actual, expected, "a new file should match File::create");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_file_atomically() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("file.txt");

        atomic_write_string(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        atomic_write_string(&path, "goodbye").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "goodbye");

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("write")
    }
}
