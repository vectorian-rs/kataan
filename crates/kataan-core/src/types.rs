use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::{vault::Vault, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct TypeDefinition {
    pub r#type: String,
    pub name: String,
    /// Every location this type may occupy, as vault-root-relative patterns.
    ///
    /// Accepts the pre-0.2 spelling `folder = "x"` as an alias, deserialized to
    /// a single-element list, so type definitions written against the old
    /// schema keep parsing untouched.
    #[serde(
        default,
        alias = "folder",
        deserialize_with = "deserialize_folder_patterns"
    )]
    pub folders: Vec<String>,
    /// The supertype, if any. Type matching walks this chain, so an edge
    /// declared `from = ["company"]` accepts a subtype of `company`.
    #[serde(default)]
    pub extends: Option<String>,
    pub icon: Option<String>,
    pub markdown: String,
    pub markdown_checksum: Option<String>,
}

impl TypeDefinition {
    /// The canonical home: the location `kataan.toml` is expected to agree
    /// with, and where a new document of this type is created by default.
    pub fn primary_folder(&self) -> Option<&str> {
        self.folders.first().map(String::as_str)
    }
}

/// Accepts either `folder = "x"` or `folders = ["x", "y"]`.
fn deserialize_folder_patterns<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(folder) => vec![folder],
        OneOrMany::Many(folders) => folders,
    })
}

/// Whether `pattern` claims `directory` or any directory beneath it.
///
/// Segment-wise, with `*` matching exactly one segment and never crossing a
/// `/`. There is deliberately no `**`: unbounded depth is what folder-level
/// scopes are for, and a `**` in the root config would silently re-create the
/// ambiguity this whole mechanism exists to remove.
///
/// A pattern matches on a *prefix* of the directory, so `companies/*/decks/*`
/// claims `companies/snappy/decks/hpc-graviton` and everything under it.
pub fn pattern_claims(pattern: &str, directory: &str) -> bool {
    let pattern_segments: Vec<&str> = split_path(pattern);
    if pattern_segments.is_empty() {
        // `"."` claims the scope itself and its whole subtree.
        return true;
    }
    let directory_segments: Vec<&str> = split_path(directory);
    if directory_segments.len() < pattern_segments.len() {
        return false;
    }
    pattern_segments
        .iter()
        .zip(directory_segments.iter())
        .all(|(pattern, segment)| *pattern == "*" || pattern == segment)
}

/// Path segments, dropping the no-op `.` and empty segments so that `"."`,
/// `""` and `"./x"` behave as callers expect.
fn split_path(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

/// Whether `pattern`, written inside `scope`, stays within that scope.
///
/// Same threat model as [`crate::index::is_safe_type_folder`]: vaults are
/// shared as git repositories, so a folder-level declaration is untrusted
/// input and must never claim documents above the folder that wrote it.
pub fn pattern_stays_in_scope(pattern: &str) -> bool {
    !std::path::Path::new(pattern).is_absolute()
        && !pattern.split('/').any(|segment| segment == "..")
}

#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    pub definitions: BTreeMap<String, TypeDefinition>,
}

impl TypeRegistry {
    pub fn contains(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }

    /// The canonical home of `name`, for callers that need one answer.
    pub fn folder_for(&self, name: &str) -> Option<&str> {
        self.definitions
            .get(name)
            .and_then(TypeDefinition::primary_folder)
    }

    /// Every location `name` may occupy.
    pub fn folders_for(&self, name: &str) -> &[String] {
        self.definitions
            .get(name)
            .map(|definition| definition.folders.as_slice())
            .unwrap_or(&[])
    }

    /// `name` and every supertype above it, nearest first.
    ///
    /// Stops on a repeat, so a cycle yields a finite chain here and is
    /// reported separately by validation rather than hanging the walk.
    pub fn ancestry(&self, name: &str) -> Vec<&str> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = self
            .definitions
            .get_key_value(name)
            .map(|(key, _)| key.as_str());
        while let Some(type_name) = current {
            if !seen.insert(type_name) {
                break;
            }
            chain.push(type_name);
            current = self
                .definitions
                .get(type_name)
                .and_then(|definition| definition.extends.as_deref())
                .and_then(|parent| self.definitions.get_key_value(parent))
                .map(|(key, _)| key.as_str());
        }
        chain
    }

    /// Whether `name` is `ancestor` or descends from it through `extends`.
    pub fn is_a(&self, name: &str, ancestor: &str) -> bool {
        name == ancestor || self.ancestry(name).contains(&ancestor)
    }

    /// Type names whose `extends` chain re-enters itself.
    pub fn extends_cycles(&self) -> Vec<&str> {
        self.definitions
            .keys()
            .filter(|name| self.is_in_cycle(name))
            .map(String::as_str)
            .collect()
    }

    /// A type is in a cycle when following `extends` from it returns to it.
    /// Types that merely lead *into* someone else's cycle are not reported, so
    /// the diagnostic names the actual offenders.
    fn is_in_cycle(&self, name: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut current = self
            .definitions
            .get(name)
            .and_then(|definition| definition.extends.as_deref());
        while let Some(type_name) = current {
            if type_name == name {
                return true;
            }
            if !seen.insert(type_name) {
                return false;
            }
            current = self
                .definitions
                .get(type_name)
                .and_then(|definition| definition.extends.as_deref());
        }
        false
    }

    pub fn load(vault: &Vault) -> Result<Self> {
        let type_folder = vault
            .index
            .type_folders
            .get("type-definition")
            .map(String::as_str)
            .unwrap_or("type");
        let type_path = vault.root.join(type_folder);
        let mut definitions = BTreeMap::new();
        if !type_path.exists() {
            return Ok(Self { definitions });
        }

        for entry in std::fs::read_dir(&type_path).map_err(|source| Error::Io {
            path: type_path.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                path: type_path.clone(),
                source,
            })?;
            let path = entry.path();
            if !path.is_file()
                || path.file_name().and_then(|name| name.to_str()) == Some("index.toml")
            {
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }

            let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let definition: TypeDefinition =
                toml::from_str(&text).map_err(|source| Error::TomlParse {
                    path: path.clone(),
                    source,
                })?;
            definitions.insert(definition.name.clone(), definition);
        }

        Ok(Self { definitions })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use crate::constants::VAULT_CONFIG_FILE;

    use super::*;

    #[test]
    fn loads_type_definitions_from_type_folder() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("type")).unwrap();
        write_root_index(&root);
        fs::write(root.join("type/project.md"), "# Project\n").unwrap();
        fs::write(
            root.join("type/project.toml"),
            r#"type = "type-definition"
name = "project"
folder = "projects"
markdown = "project.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let registry = TypeRegistry::load(&vault).unwrap();

        assert!(registry.definitions.contains_key("project"));
        assert_eq!(registry.definitions["project"].folders, vec!["projects"]);
        assert_eq!(registry.folder_for("project"), Some("projects"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_multi_folder_and_extends() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("type")).unwrap();
        write_root_index(&root);
        fs::write(root.join("type/project.md"), "# Project\n").unwrap();
        fs::write(
            root.join("type/project.toml"),
            r#"type = "type-definition"
name = "project"
folders = ["projects", "companies/*/decks/*"]
markdown = "project.md"
"#,
        )
        .unwrap();
        fs::write(root.join("type/deck.md"), "# Deck\n").unwrap();
        fs::write(
            root.join("type/deck.toml"),
            r#"type = "type-definition"
name = "deck"
extends = "project"
folders = ["companies/*/decks/*"]
markdown = "deck.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let registry = TypeRegistry::load(&vault).unwrap();

        assert_eq!(
            registry.folders_for("project"),
            ["projects", "companies/*/decks/*"]
        );
        assert_eq!(registry.folder_for("project"), Some("projects"));
        assert!(registry.is_a("deck", "project"));
        assert!(registry.is_a("deck", "deck"));
        assert!(!registry.is_a("project", "deck"));
        assert!(registry.extends_cycles().is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_extends_cycles() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("type")).unwrap();
        write_root_index(&root);
        for (name, parent) in [("a", "b"), ("b", "a")] {
            fs::write(root.join(format!("type/{name}.md")), "# X\n").unwrap();
            fs::write(
                root.join(format!("type/{name}.toml")),
                format!(
                    r#"type = "type-definition"
name = "{name}"
extends = "{parent}"
folders = ["{name}"]
markdown = "{name}.md"
"#
                ),
            )
            .unwrap();
        }

        let vault = Vault::open(&root).unwrap();
        let registry = TypeRegistry::load(&vault).unwrap();

        assert_eq!(registry.extends_cycles(), vec!["a", "b"]);
        // The ancestry walk must terminate rather than hang on the cycle.
        assert_eq!(registry.ancestry("a"), vec!["a", "b"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn patterns_claim_directories_and_their_subtrees() {
        assert!(pattern_claims("presentations", "presentations"));
        assert!(pattern_claims("presentations", "presentations/garden"));
        assert!(!pattern_claims("presentations", "companies"));

        assert!(pattern_claims(
            "companies/*/decks/*",
            "companies/snappy/decks/hpc-graviton"
        ));
        assert!(pattern_claims(
            "companies/*/decks/*",
            "companies/snappy/decks/hpc-graviton/assets"
        ));
        // `*` matches exactly one segment, so a shallower path is not claimed.
        assert!(!pattern_claims(
            "companies/*/decks/*",
            "companies/snappy/decks"
        ));
        assert!(!pattern_claims(
            "companies/*/decks/*",
            "companies/snappy/customers/x"
        ));

        // `"."` is the whole scope subtree.
        assert!(pattern_claims(".", "anything/at/all"));
    }

    #[test]
    fn rejects_patterns_that_escape_their_scope() {
        assert!(pattern_stays_in_scope("."));
        assert!(pattern_stays_in_scope("*/presentations"));
        assert!(!pattern_stays_in_scope("../siblings"));
        assert!(!pattern_stays_in_scope("/etc"));
    }

    fn write_root_index(root: &Path) {
        fs::write(
            root.join(VAULT_CONFIG_FILE),
            r#"schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
type-definition = "type"
project = "projects"
"#,
        )
        .unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        crate::test_support::unique_temp_dir("types")
    }
}
