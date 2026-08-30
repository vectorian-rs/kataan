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

    /// Resolve a filesystem path to the canonical id of the document it belongs
    /// to, for consumers that reference vault files by path rather than by id.
    ///
    /// Accepts either side of a document pair (`notes/x.md`, `notes/x.toml`), a
    /// folder's `index` pair (which resolves to the folder id), and the
    /// extensionless form (`notes/x`). The path may be vault-relative or
    /// absolute inside the vault root.
    ///
    /// Returns `None` unless the result is a document already loaded in this
    /// vault, so a path outside the root, or one naming a deleted or ignored
    /// file, cannot resolve. Traversal is rejected before the lookup:
    /// [`CanonicalId::parse`] refuses any segment containing `.`.
    pub fn resolve_path(&self, path: impl AsRef<Path>) -> Option<&CanonicalId> {
        let path = path.as_ref();
        let relative = if path.is_absolute() {
            strip_vault_root(&self.root, path)?
        } else {
            path
        };

        // `./a/b` and `a/b` name the same document.
        let cleaned: PathBuf = relative
            .components()
            .filter(|component| !matches!(component, std::path::Component::CurDir))
            .collect();
        if cleaned.as_os_str().is_empty() {
            return None;
        }

        // Documents are addressed by either file of the pair; ids themselves
        // carry no extension, so fall back to parsing the path as an id.
        let id = CanonicalId::from_document_path(&cleaned)
            .or_else(|_| CanonicalId::parse(cleaned.to_string_lossy()))
            .ok()?;
        self.documents.get_key_value(&id).map(|(id, _)| id)
    }
}

/// Make an absolute path vault-relative, tolerating a root that is stored
/// uncanonicalized while the caller passes a resolved path (or vice versa).
fn strip_vault_root<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative);
    }
    path.strip_prefix(root.canonicalize().ok()?).ok()
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
        let ignore = crate::scan::ScanIgnore::load(&self.root, &self.index.scan)?;
        self.load_documents_with_ignore(&ignore)
    }

    /// Like [`load_documents`](Self::load_documents) but reuses an already-built
    /// ignore matcher, so callers that have loaded one (e.g. validation) don't
    /// re-read `.kataanignore` and recompile the globs a second time.
    pub(crate) fn load_documents_with_ignore(
        &self,
        ignore: &crate::scan::ScanIgnore,
    ) -> Result<Vec<DocumentRecord>> {
        let mut documents = Vec::new();
        for folder in self.index.type_folders.values() {
            // Untrusted: a cloned vault could point a type folder outside its
            // own tree. `validate` reports this; loading simply skips it.
            if !crate::index::is_safe_type_folder(folder) {
                continue;
            }
            let folder_path = self.root.join(folder);
            if crate::walk::is_regular_dir(&folder_path) {
                for entry in walk_type_folder(&self.root, folder, ignore)? {
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
        if crate::walk::is_regular_file(&folder_index_toml) {
            let entry = VaultEntry::FolderIndex {
                id: id.clone(),
                markdown_path: self.root.join(id.folder_index_markdown_path()),
                toml_path: folder_index_toml,
            };
            return self.load_entry(&entry);
        }

        let toml_path = self.root.join(id.toml_path());
        let metadata = read_metadata(&toml_path)?;
        // `markdown` is attacker-controllable TOML; it must be a plain filename
        // inside the document's own folder, never a path that escapes the vault
        // (e.g. "../../etc/passwd" or "/etc/passwd").
        if !is_plain_filename(&metadata.markdown) {
            return Err(Error::InvalidVaultStructure(format!(
                "document `{id}` has an unsafe markdown path `{}`",
                metadata.markdown
            )));
        }
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
        let markdown_checksum = if crate::walk::is_regular_file(&entry.markdown_path()) {
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

/// True if `name` is a single plain filename component — no separators, no
/// `.`/`..`, not absolute — so joining it to a folder cannot escape that folder.
fn is_plain_filename(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
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

    #[test]
    fn is_plain_filename_rejects_path_traversal() {
        assert!(is_plain_filename("note.md"));
        assert!(!is_plain_filename("../note.md"));
        assert!(!is_plain_filename("../../etc/passwd"));
        assert!(!is_plain_filename("/etc/passwd"));
        assert!(!is_plain_filename(".."));
        assert!(!is_plain_filename("."));
        assert!(!is_plain_filename("sub/note.md"));
        assert!(!is_plain_filename(""));
    }

    #[test]
    fn load_document_record_rejects_unsafe_markdown() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(
            root.join("projects/evil.toml"),
            "type = \"project\"\nmarkdown = \"../../../../etc/passwd\"\n",
        )
        .unwrap();

        let vault = Vault::open(&root).unwrap();
        let id = CanonicalId::parse("projects/evil").unwrap();
        assert!(
            vault.load_document_record(&id).is_err(),
            "a markdown path that escapes the folder must be rejected"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn walk_skips_symlinked_directories() {
        let root = unique_temp_dir();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_root_index(&root);
        fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
        fs::write(
            root.join("projects/index.toml"),
            "type = \"project\"\nname = \"Projects\"\nmarkdown = \"index.md\"\n",
        )
        .unwrap();
        fs::write(root.join("projects/note.md"), "# Note\n").unwrap();
        fs::write(
            root.join("projects/note.toml"),
            "type = \"project\"\nmarkdown = \"note.md\"\n",
        )
        .unwrap();
        // A directory symlink cycle: projects/loop -> .. -> projects/loop -> ...
        // If the walkers followed symlinks this would recurse until the stack
        // overflows; the symlink guard makes it terminate and skip the link.
        std::os::unix::fs::symlink("..", root.join("projects/loop")).unwrap();

        let vault = Vault::open(&root).unwrap();
        let documents = vault.load_documents().unwrap();

        assert!(documents
            .iter()
            .any(|doc| doc.id.as_str() == "projects/note"));
        assert!(documents
            .iter()
            .all(|doc| !doc.id.as_str().contains("loop")));

        fs::remove_dir_all(root).unwrap();
    }

    /// A vault with a leaf document and a nested folder-index document, so both
    /// addressing shapes are covered.
    fn vault_with_documents(name: &str) -> std::path::PathBuf {
        let root = crate::test_support::unique_temp_dir(name);
        crate::init::init_vault(&root, "Test").unwrap();
        crate::mutate::create_document(
            &root,
            crate::mutate::NewDocument {
                r#type: "note".to_owned(),
                title: "Field Notes".to_owned(),
                body: "hello".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        root
    }

    #[test]
    fn resolve_path_accepts_every_spelling_of_one_document() {
        let root = vault_with_documents("resolve-path-forms");
        let vault = LoadedVault::load(&root).unwrap();
        let expected = CanonicalId::parse("notes/field-notes").unwrap();

        for spelling in [
            "notes/field-notes.md",
            "notes/field-notes.toml",
            "notes/field-notes",
            "./notes/field-notes.md",
        ] {
            assert_eq!(
                vault.resolve_path(spelling),
                Some(&expected),
                "`{spelling}` did not resolve"
            );
        }

        // Absolute paths inside the vault resolve too: consumers building
        // `path.join(REPO, relative)` hand us one of these.
        assert_eq!(
            vault.resolve_path(root.join("notes/field-notes.md")),
            Some(&expected)
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_path_maps_a_folder_index_to_its_folder_id() {
        let root = vault_with_documents("resolve-path-folder");
        let vault = LoadedVault::load(&root).unwrap();
        let notes = CanonicalId::parse("notes").unwrap();

        assert_eq!(vault.resolve_path("notes/index.toml"), Some(&notes));
        assert_eq!(vault.resolve_path("notes/index.md"), Some(&notes));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_path_refuses_anything_outside_the_vault() {
        let root = vault_with_documents("resolve-path-escape");
        let vault = LoadedVault::load(&root).unwrap();

        for hostile in [
            "../secrets.md",
            "notes/../../secrets.md",
            "/etc/passwd",
            "",
            ".",
        ] {
            assert_eq!(
                vault.resolve_path(hostile),
                None,
                "`{hostile}` must not resolve"
            );
        }
        // An absolute path outside the root is rejected even if it exists.
        assert_eq!(vault.resolve_path("/tmp"), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_path_returns_none_for_a_wellformed_path_to_nothing() {
        let root = vault_with_documents("resolve-path-missing");
        let vault = LoadedVault::load(&root).unwrap();

        // Shaped like a document id, but no such document — must not hand back
        // a dangling id that later lookups would fail on.
        assert_eq!(vault.resolve_path("notes/does-not-exist.md"), None);
        assert_eq!(vault.resolve_path("notes/does-not-exist"), None);

        fs::remove_dir_all(root).unwrap();
    }
}
