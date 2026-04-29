use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use crate::{
    document::DocumentMetadata,
    graph::VaultGraph,
    id::CanonicalId,
    index::{FolderIndex, VaultIndex},
    ontology::Ontology,
    types::TypeRegistry,
    walk::{walk_type_folder, VaultEntry},
    Error, Result,
};

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub index: VaultIndex,
}

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub id: CanonicalId,
    pub metadata: DocumentMetadata,
    pub markdown: String,
    pub ancestors: Vec<String>,
    pub is_folder_index: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedVault {
    pub root: PathBuf,
    pub index: VaultIndex,
    pub type_registry: TypeRegistry,
    pub ontology: Ontology,
    pub documents: BTreeMap<CanonicalId, LoadedDocument>,
    pub graph: VaultGraph,
}

impl LoadedVault {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let vault = Vault::open(root)?;
        vault.load()
    }

    pub fn get_document(&self, id: &CanonicalId) -> Option<&LoadedDocument> {
        self.documents.get(id)
    }
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index_path = root.join("index.toml");
        let index_text = std::fs::read_to_string(&index_path).map_err(|source| Error::Io {
            path: index_path.clone(),
            source,
        })?;
        let index = toml::from_str(&index_text).map_err(|source| Error::TomlParse {
            path: index_path,
            source,
        })?;
        Ok(Self { root, index })
    }

    pub fn load_folder_index(&self, folder: &str) -> Result<FolderIndex> {
        let path = self.root.join(folder).join("index.toml");
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| Error::TomlParse { path, source })
    }

    pub fn load_documents(&self) -> Result<Vec<LoadedDocument>> {
        let mut documents = Vec::new();
        for folder in self.index.type_folders.values() {
            let folder_path = self.root.join(folder);
            if folder_path.exists() {
                for entry in walk_type_folder(&self.root, folder)? {
                    documents.push(self.load_entry(&entry)?);
                }
            }
        }
        documents.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(documents)
    }

    pub fn load(&self) -> Result<LoadedVault> {
        let type_registry = TypeRegistry::load(self)?;
        let ontology = Ontology::load(&self.root)?;
        let documents = self
            .load_documents()?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();
        let graph = VaultGraph::build_with_ontology(documents.values().cloned(), Some(&ontology))?;

        Ok(LoadedVault {
            root: self.root.clone(),
            index: self.index.clone(),
            type_registry,
            ontology,
            documents,
            graph,
        })
    }

    pub fn load_graph(&self) -> Result<VaultGraph> {
        let ontology = Ontology::load(&self.root).ok();
        VaultGraph::build_with_ontology(self.load_documents()?, ontology.as_ref())
    }

    pub fn load_document(&self, id: &CanonicalId) -> Result<LoadedDocument> {
        let folder_index_toml = self.root.join(id.folder_index_toml_path());
        let is_folder_index = folder_index_toml.exists();

        let (metadata, markdown_path) = if is_folder_index {
            let toml_path = folder_index_toml;
            let toml_text = std::fs::read_to_string(&toml_path).map_err(|source| Error::Io {
                path: toml_path.clone(),
                source,
            })?;
            let metadata: DocumentMetadata =
                toml::from_str(&toml_text).map_err(|source| Error::TomlParse {
                    path: toml_path,
                    source,
                })?;
            (metadata, self.root.join(id.folder_index_markdown_path()))
        } else {
            let toml_path = self.root.join(id.toml_path());
            let toml_text = std::fs::read_to_string(&toml_path).map_err(|source| Error::Io {
                path: toml_path.clone(),
                source,
            })?;
            let metadata: DocumentMetadata =
                toml::from_str(&toml_text).map_err(|source| Error::TomlParse {
                    path: toml_path,
                    source,
                })?;
            let markdown_path = self.root.join(id.folder()).join(&metadata.markdown);
            (metadata, markdown_path)
        };

        let markdown = std::fs::read_to_string(&markdown_path).map_err(|source| Error::Io {
            path: markdown_path,
            source,
        })?;

        Ok(LoadedDocument {
            id: id.clone(),
            metadata,
            markdown,
            ancestors: id.ancestors().into_iter().map(str::to_owned).collect(),
            is_folder_index,
        })
    }

    fn load_entry(&self, entry: &VaultEntry) -> Result<LoadedDocument> {
        let toml_text = std::fs::read_to_string(entry.toml_path()).map_err(|source| Error::Io {
            path: entry.toml_path().to_path_buf(),
            source,
        })?;
        let metadata: DocumentMetadata =
            toml::from_str(&toml_text).map_err(|source| Error::TomlParse {
                path: entry.toml_path().to_path_buf(),
                source,
            })?;
        let markdown =
            std::fs::read_to_string(entry.markdown_path()).map_err(|source| Error::Io {
                path: entry.markdown_path().to_path_buf(),
                source,
            })?;
        let id = entry.id().clone();
        Ok(LoadedDocument {
            ancestors: id.ancestors().into_iter().map(str::to_owned).collect(),
            is_folder_index: entry.is_folder_index(),
            id,
            metadata,
            markdown,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn loads_folder_index() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            r#"type = "project"
name = "Projects"
description = "Project docs"
default_type = "project"
markdown = "index.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let index = vault.load_folder_index("projects").unwrap();

        assert_eq!(index.name, "Projects");
        assert_eq!(index.default_type.as_deref(), Some("project"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_graph_from_vault_documents() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            r#"type = "project"
name = "Projects"
markdown = "index.md"
"#,
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("projects/kataan-redesign")).unwrap();
        fs::write(
            root.join("projects/kataan-redesign/index.md"),
            "# Project\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign/index.toml"),
            r#"type = "project"
name = "Kataan Redesign"
markdown = "index.md"
"#,
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign/project-brief.md"),
            "# Project Brief\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign/project-brief.toml"),
            r#"type = "project"
markdown = "project-brief.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let graph = vault.load_graph().unwrap();

        let project_id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        let note_id = CanonicalId::parse("projects/kataan-redesign/project-brief").unwrap();
        assert_eq!(
            graph.children_of(&project_id),
            std::collections::BTreeSet::from([note_id])
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_semantic_loaded_vault() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("ontology.toml"),
            include_str!("../templates/default-ontology.toml"),
        )
        .unwrap();
        write_folder_doc(&root, "projects", "project", "Projects");
        fs::create_dir_all(root.join("type")).unwrap();
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

        let loaded = LoadedVault::load(&root).unwrap();
        let project_id = CanonicalId::parse("projects").unwrap();
        assert!(loaded.type_registry.contains("project"));
        assert!(loaded.ontology.edges.contains_key("related_to"));
        assert!(loaded.get_document(&project_id).is_some());
        assert!(loaded.graph.documents.contains_key(&project_id));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursively_loads_folder_index_documents_and_regular_documents() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects/company-x/internal")).unwrap();
        write_root_index(&root);
        write_folder_doc(&root, "projects", "project", "Projects");
        write_folder_doc(&root, "projects/company-x", "project", "Company X");
        write_folder_doc(&root, "projects/company-x/internal", "project", "Internal");
        fs::write(
            root.join("projects/company-x/internal/q2-launch.md"),
            "# Q2 Launch\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/company-x/internal/q2-launch.toml"),
            r#"type = "project"
markdown = "q2-launch.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let documents = vault.load_documents().unwrap();
        let ids = documents
            .iter()
            .map(|document| document.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "projects",
                "projects/company-x",
                "projects/company-x/internal",
                "projects/company-x/internal/q2-launch",
            ]
        );
        let q2 = documents
            .iter()
            .find(|document| document.id.as_str() == "projects/company-x/internal/q2-launch")
            .unwrap();
        assert_eq!(q2.ancestors, vec!["company-x", "internal"]);
        assert!(!q2.is_folder_index);
        assert!(
            documents
                .iter()
                .find(|document| document.id.as_str() == "projects/company-x")
                .unwrap()
                .is_folder_index
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_document_metadata_and_markdown() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/kataan-redesign.md"),
            "# Kataan Redesign\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/kataan-redesign.toml"),
            r#"type = "project"
status = "active"
markdown = "kataan-redesign.md"
"#,
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        let document = vault.load_document(&id).unwrap();

        assert_eq!(document.id, id);
        assert_eq!(document.metadata.r#type, "project");
        assert_eq!(document.markdown, "# Kataan Redesign\n");
        assert!(!document.is_folder_index);

        fs::remove_dir_all(root).unwrap();
    }

    fn write_folder_doc(root: &Path, folder: &str, ty: &str, title: &str) {
        fs::write(root.join(folder).join("index.md"), format!("# {title}\n")).unwrap();
        fs::write(
            root.join(folder).join("index.toml"),
            format!(
                r#"type = "{ty}"
name = "{title}"
markdown = "index.md"
"#
            ),
        )
        .unwrap();
    }

    fn write_root_index(root: &Path) {
        fs::write(
            root.join("index.toml"),
            r#"schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
project = "projects"
note = "notes"
"#,
        )
        .unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kataan-vault-test-{}-{counter}",
            std::process::id()
        ))
    }
}
