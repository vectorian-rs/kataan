//! Which files are worth putting in a full-text index, and how much of one.
//!
//! Not the same question as "can the server preview this": that asks whether
//! there is a lexer, and answers yes for anything the highlighter knows. This
//! asks whether a person would ever search for words inside the file, which is
//! narrower — a lockfile has a lexer and nothing anyone looks for.
//!
//! Measured against a real vault before choosing. With gitignore applied it
//! holds 384 JSON (mostly relationship-intelligence records), 82 `.astro`, 64
//! `.ts`, 25 `.mjs`, 11 HTML and 8 text files: about 419 files and 6.4 MB, with
//! one outlier at 1.8 MB. Doubling the item count is the real cost, and it buys
//! search over roughly half the prose in the vault.

/// Extensions worth indexing.
///
/// An allow-list, not a deny-list: a vault accumulates build output, binaries
/// and vendored trees, and guessing which of those is text means indexing a
/// `.o` file the first time one appears under a name nobody predicted.
///
/// Formatting is pinned: the groupings are how this list stays reviewable, and
/// rustfmt reflows the entries across them.
#[rustfmt::skip]
const INDEXABLE: &[&str] = &[
    // Data and prose
    "txt", "csv", "tsv", "json", "yaml", "yml", "toml", "xml", "rst", "org",
    // Markup the vault authors by hand
    "html", "htm", "astro", "svg",
    // Source
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "rb", "go", "java", "kt", "swift", "c",
    "h", "cpp", "hpp", "cs", "sh", "bash", "zsh", "fish", "sql", "css", "scss",
];

/// Files whose *name* means "generated" whatever their extension says.
///
/// A lockfile is text, has a lexer, and is pure noise in a search index: it is
/// a machine's record of resolved versions, and matching a package name in one
/// buries the document that actually discusses that package.
const GENERATED_NAMES: &[&str] = &[
    "package-lock.json",
    "bun.lock",
    "bun.lockb",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "composer.lock",
    "poetry.lock",
];

/// The most of one file that goes into the index.
///
/// Generous enough for anything hand-written — the largest text file in the
/// vault this was measured against is 1.8 MB of generated JSON — and a bound
/// on what one pathological file can do to the index.
pub(crate) const MAX_INDEXED_FILE_BYTES: u64 = 1024 * 1024;

/// Whether a file's contents belong in the index.
pub(crate) fn is_indexable(relative_path: &std::path::Path) -> bool {
    let name = relative_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if GENERATED_NAMES.contains(&name) {
        return false;
    }
    relative_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| INDEXABLE.contains(&extension.as_str()))
}

#[cfg(test)]
mod tests;
