use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use crate::{
    checksum,
    constants::VAULT_CONFIG_FILE,
    document::DocumentMetadata,
    graph::VaultGraph,
    id::CanonicalId,
    index::{FolderIndex, VaultConfig},
    ontology::Ontology,
    types::TypeRegistry,
    walk::{walk_type_folder, VaultEntry},
    Error, Result,
};

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub index: VaultConfig,
}

#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub id: CanonicalId,
    pub metadata: DocumentMetadata,
    pub markdown_path: PathBuf,
    pub toml_path: PathBuf,
    pub ancestors: Vec<String>,
    pub facets: Vec<String>,
    pub is_folder_index: bool,
    pub markdown_checksum: Option<String>,
    pub toml_checksum: String,
}

pub type LoadedDocument = DocumentRecord;

#[derive(Debug, Clone)]
pub struct DocumentContent {
    pub id: CanonicalId,
    pub metadata: DocumentMetadata,
    pub markdown: String,
    pub ancestors: Vec<String>,
    pub facets: Vec<String>,
    pub is_folder_index: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedVault {
    pub root: PathBuf,
    pub index: VaultConfig,
    pub type_registry: TypeRegistry,
    pub ontology: Ontology,
    pub documents: BTreeMap<CanonicalId, DocumentRecord>,
    pub route_tokens: HashMap<(String, String), CanonicalId>,
    pub graph: VaultGraph,
}

impl LoadedVault {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let vault = Vault::open(root)?;
        vault.load()
    }

    pub fn get_document(&self, id: &CanonicalId) -> Option<&DocumentRecord> {
        self.documents.get(id)
    }

    pub fn read_markdown(&self, id: &CanonicalId) -> Result<String> {
        let record = self
            .documents
            .get(id)
            .ok_or_else(|| Error::InvalidVaultStructure(format!("unknown document `{id}`")))?;
        std::fs::read_to_string(&record.markdown_path).map_err(|source| Error::Io {
            path: record.markdown_path.clone(),
            source,
        })
    }

    pub fn resolve_route_token(&self, type_folder: &str, token: &str) -> Option<&CanonicalId> {
        self.route_tokens
            .get(&(type_folder.to_owned(), token.to_owned()))
    }
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index_path = root.join(VAULT_CONFIG_FILE);
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

    pub fn load_documents(&self) -> Result<Vec<DocumentRecord>> {
        let mut documents = Vec::new();
        for folder in self.index.type_folders.values() {
            let folder_path = self.root.join(folder);
            if folder_path.exists() {
                for entry in walk_type_folder(&self.root, folder)? {
                    match self.load_entry(&entry) {
                        Ok(document) => documents.push(document),
                        Err(Error::TomlParse { .. }) if !entry.is_folder_index() => continue,
                        Err(error) => return Err(error),
                    }
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
        let route_tokens = documents
            .keys()
            .map(|id| {
                (
                    (id.top_level_folder().to_owned(), route_token_for_id(id)),
                    id.clone(),
                )
            })
            .collect();
        let graph = VaultGraph::build_with_ontology(documents.values().cloned(), Some(&ontology))?;

        Ok(LoadedVault {
            root: self.root.clone(),
            index: self.index.clone(),
            type_registry,
            ontology,
            documents,
            route_tokens,
            graph,
        })
    }

    pub fn load_graph(&self) -> Result<VaultGraph> {
        let ontology = Ontology::load(&self.root).ok();
        VaultGraph::build_with_ontology(self.load_documents()?, ontology.as_ref())
    }

    pub fn load_document(&self, id: &CanonicalId) -> Result<DocumentContent> {
        let record = self.load_document_record(id)?;
        let markdown =
            std::fs::read_to_string(&record.markdown_path).map_err(|source| Error::Io {
                path: record.markdown_path.clone(),
                source,
            })?;

        Ok(DocumentContent {
            id: record.id,
            metadata: record.metadata,
            markdown,
            ancestors: record.ancestors,
            facets: record.facets,
            is_folder_index: record.is_folder_index,
        })
    }

    pub fn load_document_record(&self, id: &CanonicalId) -> Result<DocumentRecord> {
        let folder_index_toml = self.root.join(id.folder_index_toml_path());
        if folder_index_toml.exists() {
            let entry = VaultEntry::FolderIndex {
                id: id.clone(),
                markdown_path: self.root.join(id.folder_index_markdown_path()),
                toml_path: folder_index_toml,
            };
            return self.load_entry(&entry);
        }

        let toml_path = self.root.join(id.toml_path());
        let metadata = read_metadata(&toml_path)?;
        let markdown_path = self.root.join(id.folder()).join(&metadata.markdown);
        let entry = VaultEntry::Document {
            id: id.clone(),
            markdown_path,
            toml_path,
        };
        self.load_entry(&entry)
    }

    fn load_entry(&self, entry: &VaultEntry) -> Result<DocumentRecord> {
        let metadata = read_metadata(entry.toml_path())?;
        let id = entry.id().clone();
        let ancestors = id
            .ancestors()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let facets = facets_for(&metadata, &ancestors);
        let markdown_checksum = if entry.markdown_path().exists() {
            Some(checksum::blake3_file(entry.markdown_path())?)
        } else {
            None
        };
        let toml_checksum = checksum::blake3_file(entry.toml_path())?;

        Ok(DocumentRecord {
            id,
            metadata,
            markdown_path: entry.markdown_path().to_path_buf(),
            toml_path: entry.toml_path().to_path_buf(),
            ancestors,
            facets,
            is_folder_index: entry.is_folder_index(),
            markdown_checksum,
            toml_checksum,
        })
    }
}

fn read_metadata(path: &Path) -> Result<DocumentMetadata> {
    let toml_text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&toml_text).map_err(|source| Error::TomlParse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn route_token_for_id(id: &CanonicalId) -> String {
    blake3::hash(id.as_str().as_bytes()).to_hex()[..32].to_owned()
}

fn facets_for(metadata: &DocumentMetadata, ancestors: &[String]) -> Vec<String> {
    let mut facets = ancestors.to_vec();
    for label in &metadata.labels {
        if !facets.contains(label) {
            facets.push(label.clone());
        }
    }
    facets
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
    fn loaded_vault_reads_markdown_on_demand() {
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
        assert_eq!(loaded.read_markdown(&project_id).unwrap(), "# Projects\n");
        fs::write(root.join("projects/index.md"), "# Changed\n").unwrap();
        assert_eq!(loaded.read_markdown(&project_id).unwrap(), "# Changed\n");

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
labels = ["launch"]
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
        assert_eq!(q2.facets, vec!["company-x", "internal", "launch"]);
        assert!(q2
            .markdown_path
            .ends_with("projects/company-x/internal/q2-launch.md"));
        assert!(q2
            .toml_path
            .ends_with("projects/company-x/internal/q2-launch.toml"));
        assert!(q2.markdown_checksum.is_some());
        assert!(!q2.toml_checksum.is_empty());
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
    fn loads_document_metadata_and_markdown_on_demand() {
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
        let record = vault.load_document_record(&id).unwrap();
        let document = vault.load_document(&id).unwrap();

        assert_eq!(record.id, id);
        assert_eq!(record.metadata.r#type, "project");
        assert!(record
            .markdown_path
            .ends_with("projects/kataan-redesign.md"));
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
            root.join(VAULT_CONFIG_FILE),
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
        crate::test_support::unique_temp_dir("vault")
    }
}
