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
        include_str!("../../templates/default-ontology.toml"),
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
        include_str!("../../templates/default-ontology.toml"),
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
