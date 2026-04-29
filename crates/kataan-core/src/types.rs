use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{vault::Vault, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct TypeDefinition {
    pub r#type: String,
    pub name: String,
    pub folder: String,
    pub markdown: String,
    pub markdown_checksum: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    pub definitions: BTreeMap<String, TypeDefinition>,
}

impl TypeRegistry {
    pub fn contains(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }

    pub fn folder_for(&self, name: &str) -> Option<&str> {
        self.definitions
            .get(name)
            .map(|definition| definition.folder.as_str())
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
        assert_eq!(registry.definitions["project"].folder, "projects");

        fs::remove_dir_all(root).unwrap();
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
