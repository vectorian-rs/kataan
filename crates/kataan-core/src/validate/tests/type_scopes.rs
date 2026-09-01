//! Folder-level type declarations, `extends`, and scope-relative depth.

use super::*;

/// A vault with the shared fixture in place. `write_root_index` writes into
/// the root before creating anything, so the directory has to exist first.
fn fresh_vault() -> PathBuf {
    let root = unique_temp_dir();
    // Every folder the shared root index maps, or validation reports the
    // missing ones and drowns the assertions in unrelated errors.
    for folder in ["intake", "projects", "people", "notes", "topics"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    write_root_index(&root);
    root
}

/// A type definition that claims nothing centrally, so only a folder-level
/// declaration can place it.
fn write_scoped_type(root: &Path, name: &str, extends: Option<&str>) {
    let extends = extends
        .map(|parent| format!("extends = \"{parent}\"\n"))
        .unwrap_or_default();
    fs::write(root.join(format!("type/{name}.md")), format!("# {name}\n")).unwrap();
    fs::write(
        root.join(format!("type/{name}.toml")),
        format!(
            "type = \"type-definition\"\nname = \"{name}\"\n{extends}markdown = \"{name}.md\"\n"
        ),
    )
    .unwrap();
}

fn write_document(root: &Path, relative: &str, document_type: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let slug = path.file_name().unwrap().to_string_lossy().to_string();
    fs::write(path.with_extension("md"), "# Document\n").unwrap();
    fs::write(
        path.with_extension("toml"),
        format!("type = \"{document_type}\"\nmarkdown = \"{slug}.md\"\n"),
    )
    .unwrap();
}

fn write_folder_scope(root: &Path, relative: &str, declarations: &str) {
    let folder = root.join(relative);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("index.md"), "# Folder\n").unwrap();
    fs::write(
        folder.join("index.toml"),
        format!(
            "type = \"project\"\nmarkdown = \"index.md\"\nname = \"Folder\"\n\n[type_folders]\n{declarations}"
        ),
    )
    .unwrap();
}

fn errors_with_code(report: &crate::diagnostic::DiagnosticReport, code: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}

#[test]
fn a_folder_scope_types_documents_below_it() {
    let root = fresh_vault();
    write_scoped_type(&root, "deck", None);
    write_folder_scope(&root, "projects/acme/decks", "deck = \".\"\n");
    write_document(&root, "projects/acme/decks/launch", "deck");
    // Depth-1 nesting under the declaring folder is covered by the same claim.
    write_document(&root, "projects/acme/decks/archive/old", "deck");
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(
        report.is_ok(),
        "expected a clean vault, got {:?}",
        report.diagnostics
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rebuild_preserves_a_folder_scope() {
    let root = fresh_vault();
    write_scoped_type(&root, "deck", None);
    write_folder_scope(&root, "projects/acme/decks", "deck = \".\"\n");
    write_document(&root, "projects/acme/decks/launch", "deck");

    // `rebuild-indexes` regenerates every index.toml from scratch, so a
    // declaration it fails to carry over would vanish on the next rebuild and
    // take the typing of everything below it with it.
    crate::rebuild::rebuild_indexes(&root).unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let rewritten = fs::read_to_string(root.join("projects/acme/decks/index.toml")).unwrap();
    assert!(
        rewritten.contains("[type_folders]") && rewritten.contains("deck = \".\""),
        "declaration was dropped by rebuild: {rewritten}"
    );
    assert!(validate(&root).unwrap().is_ok());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_type_is_not_claimed_outside_the_scope_that_declares_it() {
    let root = fresh_vault();
    write_scoped_type(&root, "deck", None);
    write_folder_scope(&root, "projects/acme/decks", "deck = \".\"\n");
    // A sibling folder, not covered by the declaration above it.
    write_document(&root, "projects/acme/notes/stray", "deck");
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(errors_with_code(&report, codes::TYPE_FOLDER_MISMATCH));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_scope_declaration_cannot_escape_its_folder() {
    let root = fresh_vault();
    write_scoped_type(&root, "deck", None);
    write_folder_scope(&root, "projects/acme/decks", "deck = \"../../..\"\n");
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(errors_with_code(&report, codes::TYPE_SCOPE_ESCAPES));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_scope_cannot_declare_an_unknown_type() {
    let root = fresh_vault();
    write_folder_scope(&root, "projects/acme/decks", "ghost = \".\"\n");
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(errors_with_code(&report, codes::TYPE_SCOPE_UNKNOWN_TYPE));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_subtype_satisfies_an_edge_rule_written_for_its_supertype() {
    let root = fresh_vault();
    // `deck` extends `project`, and the predicate only names `project`.
    write_scoped_type(&root, "deck", Some("project"));
    fs::write(
        root.join("ontology.toml"),
        r#"schema_version = "0.1.0"

[edges.related_to]
from = ["*"]
to = ["*"]
symmetric = true
cardinality = "many-to-many"

[edges.subproject_of]
from = ["project"]
to = ["project"]
cardinality = "many-to-one"
"#,
    )
    .unwrap();
    write_folder_scope(&root, "projects/acme/decks", "deck = \".\"\n");
    fs::write(root.join("projects/parent.md"), "# Parent\n").unwrap();
    fs::write(
        root.join("projects/parent.toml"),
        "type = \"project\"\nmarkdown = \"parent.md\"\n",
    )
    .unwrap();
    fs::write(root.join("projects/acme/decks/launch.md"), "# Launch\n").unwrap();
    fs::write(
        root.join("projects/acme/decks/launch.toml"),
        r#"type = "deck"
markdown = "launch.md"

[edges]
subproject_of = ["projects/parent"]
"#,
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(
        !errors_with_code(&report, codes::PREDICATE_SOURCE_TYPE_MISMATCH),
        "a subtype was refused as the source: {:?}",
        report.diagnostics
    );
    assert!(
        report.is_ok(),
        "expected a clean vault, got {:?}",
        report.diagnostics
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn depth_is_measured_from_the_nearest_scope() {
    let root = fresh_vault();
    write_scoped_type(&root, "deck", None);
    // Two levels of nesting is all the vault allows from any type folder.
    let config = fs::read_to_string(root.join(VAULT_CONFIG_FILE)).unwrap();
    fs::write(
        root.join(VAULT_CONFIG_FILE),
        config.replace(
            "name = \"Test Vault\"",
            "name = \"Test Vault\"\n\n[limits]\nmax_folder_depth = 2",
        ),
    )
    .unwrap();

    // Four levels below `projects`, so this only passes if depth restarts at
    // the declaring folder.
    write_folder_scope(&root, "projects/acme/decks", "deck = \".\"\n");
    write_document(&root, "projects/acme/decks/launch", "deck");
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();

    assert!(
        !errors_with_code(&report, codes::FOLDER_DEPTH_EXCEEDED),
        "depth was still measured from the vault root: {:?}",
        report.diagnostics
    );

    // Without a scope the same nesting is over budget, which is what makes the
    // assertion above meaningful rather than a limit that was never reached.
    let deep = root.join("projects/a/b/c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("far.md"), "# Far\n").unwrap();
    fs::write(
        deep.join("far.toml"),
        "type = \"project\"\nmarkdown = \"far.md\"\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();
    assert!(errors_with_code(&report, codes::FOLDER_DEPTH_EXCEEDED));

    fs::remove_dir_all(root).unwrap();
}
