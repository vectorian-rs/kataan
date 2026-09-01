//! Per-folder and per-document validation.
//!
//! Carved out of the parent `validate` module for file-size hygiene. The
//! orchestration in `validate.rs` drives the `pub(super)` entry points here.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    checksum::{self, SubfolderChecksum},
    constants::{ACTOR_VALUES, STATUS_VALUES},
    diagnostic::Diagnostic,
    diagnostic_codes as codes,
    document::DocumentMetadata,
    id::CanonicalId,
    index::{FolderDocument, FolderIndex, FolderSubfolder},
    scan::ScanIgnore,
    scope::{self, TypeScope},
    types::TypeRegistry,
    Result,
};

/// What stays fixed for the whole walk of one type folder.
///
/// Held together rather than passed as ten arguments because only
/// `folder_path` varies across the recursion — and because `root_folder_path`
/// and the current folder are both `&Path`, so as arguments they could be
/// swapped silently. Getting that pair wrong is what produced `projects//x`.
pub(super) struct FolderWalk<'a> {
    pub root_folder: &'a str,
    pub root_folder_path: &'a Path,
    pub max_folder_depth: usize,
    pub type_registry: &'a TypeRegistry,
    pub type_folders: &'a BTreeMap<String, String>,
    pub ignore: &'a ScanIgnore,
}

/// What the walk accumulates.
pub(super) struct Collected<'a> {
    pub issues: &'a mut Vec<Diagnostic>,
    pub known_document_types: &'a mut BTreeMap<String, String>,
    pub loaded_metadata: &'a mut Vec<(String, DocumentMetadata)>,
}

/// Depth of a document below the scope that types it.
///
/// Measured from the nearest declaring scope rather than the vault root, so
/// nesting a type deeper does not spend the depth budget on merely reaching it.
/// With no folder-level declarations the nearest scope is the root, the base is
/// the top-level type folder, and this reduces to the previous rule.
fn depth_below_scope(document_id: &str, scope_folder: &str) -> usize {
    let base = if scope_folder.is_empty() {
        1
    } else {
        scope_folder.split('/').count()
    };
    document_id.split('/').count().saturating_sub(base)
}

impl FolderWalk<'_> {
    /// The root scope: `kataan.toml [type_folders]` plus every pattern each
    /// type definition claims.
    pub(super) fn root_scope(&self) -> TypeScope {
        TypeScope::root(self.type_folders, self.type_registry)
    }

    /// Turn a folder's `[type_folders]` table into a scope, reporting the
    /// declarations that cannot be honored.
    fn scope_from_declarations(
        &self,
        issues: &mut Vec<Diagnostic>,
        relative: &str,
        declarations: &BTreeMap<String, String>,
    ) -> Option<TypeScope> {
        if declarations.is_empty() {
            return None;
        }
        let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (type_name, pattern) in declarations {
            if !self.type_registry.contains(type_name) {
                issues.push(
                    Diagnostic::error(
                        codes::TYPE_SCOPE_UNKNOWN_TYPE,
                        format!("folder declares unknown type `{type_name}`"),
                    )
                    .with_path(format!("{relative}/index.toml")),
                );
                continue;
            }
            if !crate::types::pattern_stays_in_scope(pattern) {
                issues.push(
                    Diagnostic::error(
                        codes::TYPE_SCOPE_ESCAPES,
                        format!(
                            "folder declares `{type_name} = \"{pattern}\"`, which reaches outside \
                             the folder that declared it"
                        ),
                    )
                    .with_path(format!("{relative}/index.toml")),
                );
                continue;
            }
            claims
                .entry(type_name.clone())
                .or_default()
                .push(pattern.clone());
        }
        if claims.is_empty() {
            return None;
        }
        Some(TypeScope::new(relative, claims))
    }

    pub(super) fn folder(
        &self,
        out: &mut Collected<'_>,
        folder_path: &Path,
        scopes: &mut Vec<TypeScope>,
    ) -> Result<()> {
        let relative = relative_folder_path(self.root_folder, self.root_folder_path, folder_path);
        let folder_index = validate_optional_folder_index_pair(out.issues, folder_path, &relative)?;

        // Pushed before descending, so this folder's own documents and
        // everything beneath them resolve against it. The walk visits a parent
        // before its children, so the stack is always exactly the ancestor
        // chain of the folder being validated.
        let declared = folder_index.as_ref().and_then(|index| {
            self.scope_from_declarations(out.issues, &relative, &index.type_folders)
        });
        let pushed = declared.is_some();
        if let Some(scope) = declared {
            scopes.push(scope);
        }

        // Split so the pop runs even when the walk returns early through `?`.
        let result = self.folder_contents(out, folder_path, &relative, folder_index, scopes);

        if pushed {
            scopes.pop();
        }
        result
    }

    fn folder_contents(
        &self,
        out: &mut Collected<'_>,
        folder_path: &Path,
        relative: &str,
        folder_index: Option<FolderIndex>,
        scopes: &mut Vec<TypeScope>,
    ) -> Result<()> {
        let mut markdown_slugs = BTreeSet::new();
        let mut toml_slugs = BTreeSet::new();
        let mut document_toml_files: Vec<(PathBuf, String)> = Vec::new();

        for entry in fs::read_dir(folder_path).map_err(|source| crate::Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| crate::Error::Io {
                path: folder_path.to_path_buf(),
                source,
            })?;
            let path = entry.path();

            if crate::walk::is_regular_dir(&path) {
                if self.ignore.is_ignored(&path, true) {
                    continue;
                }
                self.folder(out, &path, scopes)?;
                continue;
            }

            if !crate::walk::is_regular_file(&path) {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
                continue;
            };
            if file_name == "index.toml" || file_name == "index.md" {
                continue;
            }

            let relative_path = Path::new(&relative).join(file_name);
            let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
                continue;
            };
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            match extension {
                "md" => {
                    if CanonicalId::from_document_path(&relative_path).is_ok() {
                        markdown_slugs.insert(stem.to_owned());
                    }
                }
                "toml" if CanonicalId::from_document_path(&relative_path).is_ok() => {
                    toml_slugs.insert(stem.to_owned());
                    document_toml_files.push((path.clone(), format!("{relative}/{file_name}")));
                }
                _ => {}
            }
        }

        // A `.toml` with no matching `.md` is a standalone file, not a half-written
        // document: the vault format supports plain artifacts addressable by path,
        // which are deliberately not documents and not graph nodes. Reporting them
        // would fire on every legitimate data file a vault carries.
        document_toml_files.retain(|(path, _)| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| markdown_slugs.contains(stem))
        });

        if let Some(folder_index) = folder_index {
            validate_folder_index(
                out.issues,
                relative,
                folder_path,
                &folder_index,
                &markdown_slugs,
                &toml_slugs,
                self.ignore,
            )?;
        }

        for (toml_path, relative_toml_path) in document_toml_files {
            self.document(out, &toml_path, &relative_toml_path, scopes)?;
        }

        Ok(())
    }
}

fn validate_optional_folder_index_pair(
    issues: &mut Vec<Diagnostic>,
    folder_path: &Path,
    relative: &str,
) -> Result<Option<FolderIndex>> {
    let folder_index_path = folder_path.join("index.toml");
    let folder_markdown_path = folder_path.join("index.md");
    match (folder_markdown_path.exists(), folder_index_path.exists()) {
        (true, true) => read_folder_index_with_diagnostic(issues, folder_path, relative),
        (true, false) => {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_FOLDER_INDEX,
                    "Folder has index.md but is missing index.toml",
                )
                .with_path(format!("{relative}/index.toml")),
            );
            Ok(None)
        }
        (false, true) => {
            issues.push(
                Diagnostic::error(
                    codes::MISSING_MARKDOWN_FILE,
                    "Folder has index.toml but is missing index.md",
                )
                .with_path(format!("{relative}/index.md")),
            );
            read_folder_index_with_diagnostic(issues, folder_path, relative)
        }
        (false, false) => Ok(None),
    }
}

fn validate_metadata_markdown_path(
    issues: &mut Vec<Diagnostic>,
    toml_path: &Path,
    relative_toml_path: &str,
    metadata: &DocumentMetadata,
) -> bool {
    let Some(stem) = toml_path.file_stem().and_then(|stem| stem.to_str()) else {
        return true;
    };
    let expected_markdown = format!("{stem}.md");
    if metadata.markdown == expected_markdown {
        return true;
    }

    issues.push(
        Diagnostic::error(
            codes::MARKDOWN_PATH_MISMATCH,
            format!(
                "metadata.markdown must point to `{expected_markdown}`, not `{}`",
                metadata.markdown
            ),
        )
        .with_path(relative_toml_path.to_owned()),
    );
    false
}

/// The vault-relative path of a folder, e.g. `projects/company-x`.
///
/// At depth 0 the suffix is empty, and `Path::join("")` appends a separator —
/// which would yield `projects/` and then `projects//x.toml` for every document
/// directly inside a type folder.
fn relative_folder_path(root_folder: &str, root_folder_path: &Path, folder_path: &Path) -> String {
    let suffix = folder_path
        .strip_prefix(root_folder_path)
        .unwrap_or(Path::new(""));
    if suffix.as_os_str().is_empty() {
        return root_folder.to_owned();
    }
    Path::new(root_folder)
        .join(suffix)
        .to_string_lossy()
        .replace('\\', "/")
}

impl FolderWalk<'_> {
    fn document(
        &self,
        out: &mut Collected<'_>,
        toml_path: &Path,
        relative_toml_path: &str,
        scopes: &[TypeScope],
    ) -> Result<()> {
        let toml_text = fs::read_to_string(toml_path).map_err(|source| crate::Error::Io {
            path: toml_path.to_path_buf(),
            source,
        })?;
        let metadata: DocumentMetadata = match toml::from_str(&toml_text) {
            Ok(metadata) => metadata,
            Err(source) => {
                out.issues.push(
                    Diagnostic::error(codes::INVALID_TOML, source.to_string())
                        .with_path(relative_toml_path.to_owned()),
                );
                return Ok(());
            }
        };

        let Some(document_id) = relative_toml_path.strip_suffix(".toml") else {
            return Ok(());
        };
        let document_id = document_id.to_owned();
        out.known_document_types
            .insert(document_id.clone(), metadata.r#type.clone());
        out.loaded_metadata
            .push((relative_toml_path.to_owned(), metadata.clone()));

        let document_directory = document_id
            .rsplit_once('/')
            .map(|(directory, _)| directory)
            .unwrap_or("");
        let scope_folder = scope::nearest_folder(scopes, document_directory);

        if CanonicalId::parse(&document_id).is_ok()
            && depth_below_scope(&document_id, scope_folder) > self.max_folder_depth
        {
            out.issues.push(
                Diagnostic::error(
                    codes::FOLDER_DEPTH_EXCEEDED,
                    format!(
                        "document depth exceeds max_folder_depth `{}`",
                        self.max_folder_depth
                    ),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        }

        if let Some(status) = &metadata.status {
            if !STATUS_VALUES.contains(&status.as_str()) {
                out.issues.push(
                    Diagnostic::error(codes::INVALID_STATUS, format!("unknown status `{status}`"))
                        .with_path(relative_toml_path.to_owned()),
                );
            }
        }

        validate_timestamps(out.issues, &metadata, relative_toml_path);

        for (field, actor) in [
            ("created_by", metadata.created_by.as_deref()),
            ("last_updated_by", metadata.last_updated_by.as_deref()),
        ] {
            if let Some(actor) = actor {
                if !ACTOR_VALUES.contains(&actor) {
                    out.issues.push(
                        Diagnostic::error(
                            codes::INVALID_ACTOR,
                            format!("{field} has unknown actor `{actor}`"),
                        )
                        .with_path(relative_toml_path.to_owned()),
                    );
                }
            }
        }

        // A type is legal here when any scope in the ancestor chain claims
        // this directory for it. With no folder-level declarations the chain is
        // just the root, and this is the previous top-level folder check.
        let known_type = self.type_registry.contains(&metadata.r#type)
            || self.type_folders.contains_key(&metadata.r#type);
        if !known_type {
            out.issues.push(
                Diagnostic::error(
                    codes::INVALID_TYPE,
                    format!("unknown type `{}`", metadata.r#type),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        } else if !scope::is_claimed(scopes, &metadata.r#type, document_directory) {
            let claims = scope::describe_claims(scopes, &metadata.r#type);
            out.issues.push(
                Diagnostic::error(
                    codes::TYPE_FOLDER_MISMATCH,
                    format!(
                        "document type `{}` is not claimed at `{document_directory}`; claims: {claims}",
                        metadata.r#type
                    ),
                )
                .with_path(relative_toml_path.to_owned()),
            );
        }

        if !validate_metadata_markdown_path(out.issues, toml_path, relative_toml_path, &metadata) {
            return Ok(());
        }
        let markdown_path = toml_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(&metadata.markdown);
        if markdown_path.exists() {
            if let Some(expected_checksum) = metadata.markdown_checksum {
                let actual_checksum = checksum::blake3_file(&markdown_path)?;
                if actual_checksum != expected_checksum {
                    out.issues.push(
                        Diagnostic::error(
                            codes::CHECKSUM_MISMATCH,
                            "Markdown checksum does not match file contents",
                        )
                        .with_path(relative_toml_path.to_owned()),
                    );
                }
            }
        }

        Ok(())
    }
}

fn read_folder_index_with_diagnostic(
    issues: &mut Vec<Diagnostic>,
    folder_path: &Path,
    relative: &str,
) -> Result<Option<FolderIndex>> {
    let path = folder_path.join("index.toml");
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|source| crate::Error::Io {
        path: path.clone(),
        source,
    })?;
    match toml::from_str::<FolderIndex>(&text) {
        Ok(index) => Ok(Some(index)),
        Err(source) => {
            issues.push(
                Diagnostic::error(codes::INVALID_TOML, source.to_string())
                    .with_path(format!("{relative}/index.toml")),
            );
            Ok(None)
        }
    }
}

fn read_folder_index_if_present(folder_path: &Path) -> Result<Option<FolderIndex>> {
    let path = folder_path.join("index.toml");
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|source| crate::Error::Io {
        path: path.clone(),
        source,
    })?;
    Ok(toml::from_str::<FolderIndex>(&text).ok())
}

fn validate_folder_index(
    issues: &mut Vec<Diagnostic>,
    folder: &str,
    folder_path: &Path,
    folder_index: &FolderIndex,
    markdown_slugs: &BTreeSet<String>,
    toml_slugs: &BTreeSet<String>,
    ignore: &ScanIgnore,
) -> Result<()> {
    let actual_slugs = markdown_slugs
        .intersection(toml_slugs)
        .cloned()
        .collect::<BTreeSet<_>>();
    let indexed_slugs = folder_index
        .documents
        .iter()
        .map(|document| document.slug.clone())
        .collect::<BTreeSet<_>>();

    for slug in actual_slugs.difference(&indexed_slugs) {
        issues.push(
            Diagnostic::warning(codes::INDEX_DRIFT, "Document is missing from folder index")
                .with_path(format!("{folder}/{slug}.md")),
        );
    }

    for _slug in indexed_slugs.difference(&actual_slugs) {
        issues.push(
            Diagnostic::warning(
                codes::INDEX_DRIFT,
                "Folder index contains a stale document entry",
            )
            .with_path(format!("{folder}/index.toml")),
        );
    }

    let actual_subfolders = actual_subfolders(folder_path, ignore)?;
    let indexed_subfolders = folder_index
        .subfolders
        .iter()
        .map(|subfolder| subfolder.name.clone())
        .collect::<BTreeSet<_>>();
    let actual_subfolder_names = actual_subfolders
        .iter()
        .map(|subfolder| subfolder.name.clone())
        .collect::<BTreeSet<_>>();

    for name in actual_subfolder_names.difference(&indexed_subfolders) {
        issues.push(
            Diagnostic::warning(codes::INDEX_DRIFT, "Subfolder is missing from folder index")
                .with_path(format!("{folder}/{name}/index.toml")),
        );
    }

    for name in indexed_subfolders.difference(&actual_subfolder_names) {
        issues.push(
            Diagnostic::warning(
                codes::INDEX_DRIFT,
                format!("Folder index contains stale subfolder entry `{name}`"),
            )
            .with_path(format!("{folder}/index.toml")),
        );
    }

    let mut actual_documents = Vec::new();
    for document in &folder_index.documents {
        let markdown_path = folder_path.join(&document.markdown);
        let toml_path = folder_path.join(&document.toml);
        if !markdown_path.exists() || !toml_path.exists() {
            continue;
        }

        let actual_markdown_checksum = checksum::blake3_file(&markdown_path)?;
        if actual_markdown_checksum != document.markdown_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder index Markdown checksum does not match file contents",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }

        let actual_toml_checksum = checksum::blake3_file(&toml_path)?;
        if actual_toml_checksum != document.toml_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder index TOML checksum does not match file contents",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }

        actual_documents.push(FolderDocument {
            slug: document.slug.clone(),
            markdown: document.markdown.clone(),
            toml: document.toml.clone(),
            markdown_checksum: actual_markdown_checksum,
            toml_checksum: actual_toml_checksum,
        });
    }

    if let Some(expected_folder_checksum) = &folder_index.folder_checksum {
        let checksum_subfolders = actual_subfolders
            .iter()
            .map(|subfolder| SubfolderChecksum {
                name: subfolder.name.clone(),
                folder_checksum: subfolder.folder_checksum.clone(),
            })
            .collect::<Vec<_>>();
        let actual_folder_checksum =
            checksum::folder_checksum_recursive(&actual_documents, &checksum_subfolders);
        if &actual_folder_checksum != expected_folder_checksum {
            issues.push(
                Diagnostic::error(
                    codes::CHECKSUM_MISMATCH,
                    "Folder checksum does not match indexed document checksums",
                )
                .with_path(format!("{folder}/index.toml")),
            );
        }
    }

    Ok(())
}

fn actual_subfolders(folder_path: &Path, ignore: &ScanIgnore) -> Result<Vec<FolderSubfolder>> {
    let mut subfolders = Vec::new();
    for entry in fs::read_dir(folder_path).map_err(|source| crate::Error::Io {
        path: folder_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| crate::Error::Io {
            path: folder_path.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !crate::walk::is_regular_dir(&path)
            || ignore.is_ignored(&path, true)
            || !path.join("index.md").exists()
            || !path.join("index.toml").exists()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let index = read_folder_index_if_present(&path)?;
        if let Some(folder_checksum) = index.and_then(|index| index.folder_checksum) {
            subfolders.push(FolderSubfolder {
                name,
                folder_checksum,
            });
        }
    }
    subfolders.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(subfolders)
}

/// Check the three first-class time fields, reporting a distinct code per
/// failure mode.
fn validate_timestamps(
    issues: &mut Vec<Diagnostic>,
    metadata: &DocumentMetadata,
    relative_toml_path: &str,
) {
    for (field, value) in [
        ("occurred_at", metadata.occurred_at.as_deref()),
        ("created_at", metadata.created_at.as_deref()),
        ("updated_at", metadata.updated_at.as_deref()),
    ] {
        let Some(value) = value else { continue };
        if let Err(error) = crate::time::Timestamp::parse(value) {
            let code = match error {
                crate::time::TimestampError::UnixEpoch(_) => codes::UNIX_EPOCH_TIMESTAMP,
                crate::time::TimestampError::Zoneless(_) => codes::ZONELESS_TIMESTAMP,
                crate::time::TimestampError::Unparseable(_) => codes::INVALID_TIMESTAMP,
            };
            issues.push(
                Diagnostic::error(code, format!("`{field}`: {error}"))
                    .with_path(relative_toml_path.to_owned()),
            );
        }
    }
}
