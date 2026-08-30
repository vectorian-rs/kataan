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
        let markdown_checksum = if crate::walk::is_regular_file(entry.markdown_path()) {
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
mod tests;
