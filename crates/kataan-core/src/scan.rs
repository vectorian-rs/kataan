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
    warnings: Vec<String>,
}

impl ScanIgnore {
    /// Build the matcher for `root` from its scan configuration.
    ///
    /// Invalid `[scan] ignore` patterns and `.kataanignore` lines are skipped
    /// and collected as [`warnings`](Self::warnings) rather than aborting the
    /// scan, so a config typo cannot take down validation or index rebuilds.
    pub fn load(root: &Path, config: &ScanConfig) -> Result<Self> {
        let mut builder = GitignoreBuilder::new(root);
        // Match ignore names case-insensitively on filesystems that are (macOS,
        // Windows), the way git auto-sets core.ignorecase, so `Node_Modules`
        // still prunes.
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            builder.case_insensitive(true).map_err(scan_error)?;
        }
        let mut warnings = Vec::new();
        if config.use_default_ignores {
            for name in DEFAULT_IGNORED_DIRS {
                // Defaults are static and known-valid; ignore defensively.
                let _ = builder.add_line(None, name);
            }
        }
        for pattern in &config.ignore {
            add_checked(&mut builder, root, pattern, "[scan] ignore", &mut warnings);
        }
        let kataanignore = root.join(KATAANIGNORE_FILE);
        if kataanignore.is_file() {
            match std::fs::read_to_string(&kataanignore) {
                Ok(text) => {
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() || trimmed.starts_with('#') {
                            continue;
                        }
                        add_checked(&mut builder, root, line, KATAANIGNORE_FILE, &mut warnings);
                    }
                }
                Err(error) => warnings.push(format!("cannot read {KATAANIGNORE_FILE}: {error}")),
            }
        }
        let gitignore = builder.build().map_err(scan_error)?;
        Ok(Self {
            root: root.to_path_buf(),
            gitignore,
            warnings,
        })
    }

    /// A matcher that ignores nothing, rooted at `root` (tests, fallbacks).
    pub fn none(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            gitignore: Gitignore::empty(),
            warnings: Vec::new(),
        }
    }

    /// Human-readable messages for ignore patterns that could not be compiled.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Whether `path` (a descendant of the vault root) should be skipped.
    /// Checked at each directory during descent, so pruning a directory keeps
    /// the walker from ever entering its subtree.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        self.gitignore.matched(relative, is_dir).is_ignore()
    }
}

/// Add `pattern` to `builder` only if it compiles on its own. A bad glob is
/// recorded in `warnings` and skipped, because `GitignoreBuilder` defers glob
/// compilation to `build()`, so a single invalid line would otherwise poison
/// the whole matcher.
fn add_checked(
    builder: &mut GitignoreBuilder,
    root: &Path,
    pattern: &str,
    source: &str,
    warnings: &mut Vec<String>,
) {
    if let Err(error) = compile_probe(root, pattern) {
        warnings.push(format!(
            "{source}: invalid ignore pattern `{pattern}`: {error}"
        ));
        return;
    }
    let _ = builder.add_line(None, pattern);
}

/// Compile `pattern` in an isolated builder to surface parse errors eagerly.
fn compile_probe(root: &Path, pattern: &str) -> std::result::Result<(), ignore::Error> {
    let mut probe = GitignoreBuilder::new(root);
    probe.add_line(None, pattern)?;
    probe.build()?;
    Ok(())
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
    fn invalid_pattern_is_collected_as_warning_not_error() {
        let root = Path::new("/vault");
        let config = ScanConfig {
            // Unclosed alternate group `{`: an invalid glob.
            ignore: vec!["a{b,c".to_owned(), "node_modules".to_owned()],
            use_default_ignores: false,
        };

        let ignore = ScanIgnore::load(root, &config).unwrap();

        assert_eq!(ignore.warnings().len(), 1);
        // The valid pattern still applies despite the invalid one.
        assert!(ignore.is_ignored(&root.join("a/node_modules"), true));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn matches_case_insensitively_on_case_insensitive_platforms() {
        let root = Path::new("/vault");
        let ignore = ScanIgnore::load(root, &ScanConfig::default()).unwrap();
        assert!(ignore.is_ignored(&root.join("a/Node_Modules"), true));
        assert!(ignore.is_ignored(&root.join("TARGET"), true));
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
