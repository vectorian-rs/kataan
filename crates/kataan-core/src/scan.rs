//! Directory-scan ignore rules for vault enumeration.
//!
//! The walkers (validate, rebuild-indexes, document loading) prune ignored
//! directories so build/vendor trees like `node_modules/` never produce
//! diagnostics or land in generated indexes or folder checksums. Defaults can
//! be extended via the `[scan]` section of kataan.toml and a `.kataanignore`
//! file at the vault root (both gitignore syntax).

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::{index::ScanConfig, Error, Result};

/// Directory names pruned by default, matched by name at any depth.
pub const DEFAULT_IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    "target",
    "dist",
    "build",
    ".astro",
    ".next",
    ".cache",
    ".venv",
    "venv",
    "__pycache__",
    ".DS_Store",
];

/// Filename, at the vault root, holding extra gitignore-style patterns.
pub const KATAANIGNORE_FILE: &str = ".kataanignore";

/// Compiled ignore matcher for a vault: the built-in defaults plus the
/// `[scan] ignore` patterns from kataan.toml plus `.kataanignore`.
#[derive(Debug, Clone)]
pub struct ScanIgnore {
    root: PathBuf,
    gitignore: Gitignore,
}

impl ScanIgnore {
    /// Build the matcher for `root` from its scan configuration.
    pub fn load(root: &Path, config: &ScanConfig) -> Result<Self> {
        let mut builder = GitignoreBuilder::new(root);
        if config.use_default_ignores {
            for name in DEFAULT_IGNORED_DIRS {
                add_pattern(&mut builder, name)?;
            }
        }
        for pattern in &config.ignore {
            add_pattern(&mut builder, pattern)?;
        }
        let kataanignore = root.join(KATAANIGNORE_FILE);
        if kataanignore.is_file() {
            if let Some(error) = builder.add(&kataanignore) {
                return Err(scan_error(error));
            }
        }
        let gitignore = builder.build().map_err(scan_error)?;
        Ok(Self {
            root: root.to_path_buf(),
            gitignore,
        })
    }

    /// A matcher that ignores nothing, rooted at `root` (tests, fallbacks).
    pub fn none(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            gitignore: Gitignore::empty(),
        }
    }

    /// Whether `path` (a descendant of the vault root) should be skipped.
    /// Checked at each directory during descent, so pruning a directory keeps
    /// the walker from ever entering its subtree.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        self.gitignore.matched(relative, is_dir).is_ignore()
    }
}

fn add_pattern(builder: &mut GitignoreBuilder, pattern: &str) -> Result<()> {
    builder
        .add_line(None, pattern)
        .map(|_| ())
        .map_err(scan_error)
}

fn scan_error(error: ignore::Error) -> Error {
    Error::InvalidVaultStructure(format!("invalid scan ignore pattern: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_default_dirs_at_any_depth() {
        let root = Path::new("/vault");
        let ignore = ScanIgnore::load(root, &ScanConfig::default()).unwrap();
        assert!(ignore.is_ignored(&root.join("a/b/node_modules"), true));
        assert!(ignore.is_ignored(&root.join("target"), true));
        assert!(!ignore.is_ignored(&root.join("companies/snappy"), true));
    }

    #[test]
    fn honors_kataanignore_file() {
        let root = crate::test_support::unique_temp_dir("scan");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(KATAANIGNORE_FILE), "secret/\n").unwrap();

        let ignore = ScanIgnore::load(&root, &ScanConfig::default()).unwrap();
        assert!(ignore.is_ignored(&root.join("a/b/secret"), true));
        assert!(!ignore.is_ignored(&root.join("public"), true));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn honors_config_patterns_and_can_drop_defaults() {
        let root = Path::new("/vault");
        let config = ScanConfig {
            ignore: vec!["**/*.tmp".to_owned(), "some/specific/dir".to_owned()],
            use_default_ignores: false,
        };
        let ignore = ScanIgnore::load(root, &config).unwrap();
        assert!(ignore.is_ignored(&root.join("a/scratch.tmp"), false));
        assert!(ignore.is_ignored(&root.join("some/specific/dir"), true));
        // defaults dropped
        assert!(!ignore.is_ignored(&root.join("node_modules"), true));
    }
}
