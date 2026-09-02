//! Where a type is allowed to live.
//!
//! A type's homes come from three places: `kataan.toml [type_folders]`, the
//! `folders` patterns on its type definition, and `[type_folders]` tables in
//! folder indexes, which apply to that folder's subtree only. Resolving a type
//! against a path means asking all of them.
//!
//! Shared rather than private to the validator because creation has to answer
//! the same question. Two implementations of "may this type live here" would
//! drift, and the direction they drift is a vault that validates but cannot be
//! written to.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{types::TypeRegistry, Error, Result};

/// One level of declarations: the vault root, or a folder that declares types
/// for its own subtree.
///
/// Claims are stored as written, relative to `folder`, and resolved on lookup
/// so a diagnostic can quote what the author actually typed.
#[derive(Debug, Clone)]
pub struct TypeScope {
    folder: String,
    claims: BTreeMap<String, Vec<String>>,
}

impl TypeScope {
    pub fn new(folder: impl Into<String>, claims: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            folder: folder.into(),
            claims,
        }
    }

    /// The root scope: every location a type definition claims, plus any
    /// `kataan.toml` mapping for a type that has no definition.
    ///
    /// Validation already requires the `kataan.toml` mapping to be among a
    /// definition's `folders`, so merging both wholesale would only produce
    /// duplicates. The fallback matters solely for a type mapped in the config
    /// with no definition behind it, which is itself a reported error but
    /// should not also mistype every document under it.
    pub fn root(type_folders: &BTreeMap<String, String>, registry: &TypeRegistry) -> Self {
        let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (type_name, definition) in &registry.definitions {
            if !definition.folders.is_empty() {
                claims.insert(type_name.clone(), definition.folders.clone());
            }
        }
        for (type_name, folder) in type_folders {
            claims
                .entry(type_name.clone())
                .or_insert_with(|| vec![folder.clone()]);
        }
        Self {
            folder: String::new(),
            claims,
        }
    }

    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// This scope's claims for `type_name`, as vault-relative patterns.
    fn resolved(&self, type_name: &str) -> Vec<String> {
        self.claims
            .get(type_name)
            .map(|patterns| {
                patterns
                    .iter()
                    .map(|pattern| join(&self.folder, pattern))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn describe(&self, type_name: &str) -> Vec<String> {
        let source = if self.folder.is_empty() {
            "root"
        } else {
            self.folder.as_str()
        };
        self.claims
            .get(type_name)
            .map(|patterns| {
                patterns
                    .iter()
                    .map(|pattern| format!("`{pattern}` ({source})"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `directory` is inside this scope.
    fn contains(&self, directory: &str) -> bool {
        self.folder.is_empty()
            || directory == self.folder
            || directory.starts_with(&format!("{}/", self.folder))
    }
}

/// A scope-relative pattern as a vault-relative one.
fn join(folder: &str, pattern: &str) -> String {
    let pattern = pattern.trim_start_matches("./");
    let pattern_is_self = pattern.is_empty() || pattern == ".";
    match (folder.is_empty(), pattern_is_self) {
        (true, true) => String::new(),
        (true, false) => pattern.to_owned(),
        (false, true) => folder.to_owned(),
        (false, false) => format!("{folder}/{pattern}"),
    }
}

/// Whether any scope claims `directory` for `type_name`.
///
/// A union, deliberately: a deck genuinely does belong in more than one place,
/// so legality is permissive. Picking a *default* home is the narrower question
/// [`default_home`] answers.
pub fn is_claimed(scopes: &[TypeScope], type_name: &str, directory: &str) -> bool {
    scopes.iter().any(|scope| {
        scope
            .resolved(type_name)
            .iter()
            .any(|pattern| crate::types::pattern_claims(pattern, directory))
    })
}

/// The folder of the innermost scope containing `directory`.
///
/// Scopes are ordered outermost first, so the last match is the nearest. The
/// root scope contains everything, so with a non-empty chain there is always an
/// answer.
pub fn nearest_folder<'a>(scopes: &'a [TypeScope], directory: &str) -> &'a str {
    scopes
        .iter()
        .rev()
        .find(|scope| scope.contains(directory))
        .map(TypeScope::folder)
        .unwrap_or("")
}

/// Every claim for `type_name`, formatted for a diagnostic.
pub fn describe_claims(scopes: &[TypeScope], type_name: &str) -> String {
    let claims: Vec<String> = scopes
        .iter()
        .flat_map(|scope| scope.describe(type_name))
        .collect();
    if claims.is_empty() {
        "none".to_owned()
    } else {
        claims.join(", ")
    }
}

/// Where a new document of `type_name` goes when the caller names no parent.
///
/// Only a claim with no wildcard can serve: `companies/*/decks/*` describes
/// where decks are allowed, not a directory anything can be created in. A type
/// placed solely by patterns therefore requires an explicit parent, which is
/// the honest answer rather than inventing a path.
pub fn default_home(scopes: &[TypeScope], type_name: &str) -> Option<String> {
    scopes
        .iter()
        .flat_map(|scope| scope.resolved(type_name))
        .find(|pattern| !pattern.is_empty() && !pattern.contains('*'))
}

/// The scope chain for one directory, read from disk.
///
/// The validator accumulates the same chain as it walks, since it visits a
/// parent before its children. Callers that need the answer for a single path,
/// such as document creation, cannot walk the whole vault to get it.
pub fn chain_for(
    root: &Path,
    type_folders: &BTreeMap<String, String>,
    registry: &TypeRegistry,
    directory: &str,
) -> Result<Vec<TypeScope>> {
    let mut scopes = vec![TypeScope::root(type_folders, registry)];
    let mut prefix = PathBuf::new();
    let mut relative = String::new();
    for segment in directory.split('/').filter(|s| !s.is_empty()) {
        prefix.push(segment);
        if relative.is_empty() {
            relative = segment.to_owned();
        } else {
            relative.push('/');
            relative.push_str(segment);
        }
        let index_path = root.join(&prefix).join("index.toml");
        if !index_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&index_path).map_err(|source| Error::Io {
            path: index_path.clone(),
            source,
        })?;
        let index: crate::index::FolderIndex =
            toml::from_str(&text).map_err(|source| Error::TomlParse {
                path: index_path,
                source,
            })?;
        if index.type_folders.is_empty() {
            continue;
        }
        let claims = index
            .type_folders
            .iter()
            .filter(|(_, pattern)| crate::types::pattern_stays_in_scope(pattern))
            .map(|(ty, pattern)| (ty.clone(), vec![pattern.clone()]))
            .collect();
        scopes.push(TypeScope::new(relative.clone(), claims));
    }
    Ok(scopes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(type_name: &str, folders: &[&str]) -> TypeRegistry {
        let mut registry = TypeRegistry::default();
        registry.definitions.insert(
            type_name.to_owned(),
            crate::types::TypeDefinition {
                r#type: "type-definition".to_owned(),
                name: type_name.to_owned(),
                folders: folders.iter().map(|f| (*f).to_owned()).collect(),
                extends: None,
                icon: None,
                markdown: format!("{type_name}.md"),
                markdown_checksum: None,
            },
        );
        registry
    }

    fn type_folders() -> BTreeMap<String, String> {
        BTreeMap::from([("note".to_owned(), "notes".to_owned())])
    }

    #[test]
    fn a_wildcard_claim_is_not_a_default_home() {
        let registry = registry_with("deck", &["companies/*/decks/*"]);
        let scopes = vec![TypeScope::root(&type_folders(), &registry)];

        assert!(is_claimed(&scopes, "deck", "companies/snappy/decks/x"));
        // Allowed to live there, but nothing can be created without a parent.
        assert_eq!(default_home(&scopes, "deck"), None);
        // A type mapped in kataan.toml with no definition still resolves.
        assert_eq!(default_home(&scopes, "note"), Some("notes".to_owned()));
    }

    #[test]
    fn a_folder_scope_extends_the_chain() {
        let registry = registry_with("deck", &[]);
        let root_scope = TypeScope::root(&type_folders(), &registry);
        let local = TypeScope::new(
            "companies/snappy/customers",
            BTreeMap::from([("deck".to_owned(), vec!["*/presentations".to_owned()])]),
        );
        let scopes = vec![root_scope, local];

        assert!(is_claimed(
            &scopes,
            "deck",
            "companies/snappy/customers/fe/presentations"
        ));
        assert!(!is_claimed(
            &scopes,
            "deck",
            "companies/snappy/customers/fe/opex"
        ));
        assert_eq!(
            nearest_folder(&scopes, "companies/snappy/customers/fe/presentations"),
            "companies/snappy/customers"
        );
        assert_eq!(nearest_folder(&scopes, "notes"), "");
    }
}
