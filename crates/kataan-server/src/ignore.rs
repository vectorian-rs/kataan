use std::path::{Path, PathBuf};

use ::ignore::{gitignore::Gitignore, Match};
use tracing::debug;

pub struct VaultIgnore {
    root: PathBuf,
    gitignore: Option<Gitignore>,
}

impl std::fmt::Debug for VaultIgnore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultIgnore")
            .field("root", &self.root)
            .field("has_gitignore", &self.gitignore.is_some())
            .finish()
    }
}

impl VaultIgnore {
    /// A matcher that applies only the built-in ignores (no `.gitignore`). Used
    /// as a graceful fallback when the vault's `.gitignore` cannot be compiled.
    pub fn empty(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            gitignore: None,
        }
    }

    pub fn load(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let gitignore_path = root.join(".gitignore");
        let gitignore = if gitignore_path.is_file() {
            let mut builder = ::ignore::gitignore::GitignoreBuilder::new(&root);
            if let Some(error) = builder.add(&gitignore_path) {
                debug!(path = %gitignore_path.display(), error = %error, "failed to load vault .gitignore");
            }
            Some(builder.build()?)
        } else {
            None
        };

        Ok(Self { root, gitignore })
    }

    pub fn should_ignore_path(&self, path: &Path) -> bool {
        if is_builtin_ignore_path(&self.root, path) {
            return true;
        }

        // Always observe the root .gitignore itself so edits to ignore rules trigger a reload.
        if path == self.root.join(".gitignore") {
            return false;
        }

        let Some(gitignore) = &self.gitignore else {
            return false;
        };

        if path.exists() {
            return is_ignore_match(gitignore.matched_path_or_any_parents(path, path.is_dir()));
        }

        // Deleted paths no longer have file type metadata. Check both file and directory
        // matching so directory-only patterns still suppress remove events for ignored dirs.
        is_ignore_match(gitignore.matched_path_or_any_parents(path, false))
            || is_ignore_match(gitignore.matched_path_or_any_parents(path, true))
    }
}

fn is_ignore_match(matched: Match<&::ignore::gitignore::Glob>) -> bool {
    matches!(matched, Match::Ignore(_))
}

fn is_builtin_ignore_path(root: &Path, path: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kataan-server-ignore-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn respects_root_gitignore_patterns() {
        let root = temp_dir("patterns");
        std::fs::write(root.join(".gitignore"), "scratch/\n*.tmp\n!important.tmp\n").unwrap();
        std::fs::create_dir_all(root.join("scratch")).unwrap();
        std::fs::write(root.join("note.tmp"), "temp").unwrap();
        std::fs::write(root.join("important.tmp"), "keep").unwrap();

        let ignore = VaultIgnore::load(&root).unwrap();

        assert!(ignore.should_ignore_path(&root.join("scratch")));
        assert!(ignore.should_ignore_path(&root.join("scratch/file.md")));
        assert!(ignore.should_ignore_path(&root.join("note.tmp")));
        assert!(!ignore.should_ignore_path(&root.join("important.tmp")));
        assert!(!ignore.should_ignore_path(&root.join(".gitignore")));

        let _ = std::fs::remove_dir_all(root);
    }
}
