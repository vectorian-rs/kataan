use std::{
    fs,
    path::{Path, PathBuf},
};

use super::*;

#[test]
fn reports_folder_index_checksum_mismatch() {
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
markdown = "kataan-redesign.md"
"#,
    )
    .unwrap();
    fs::write(
        root.join("projects/index.toml"),
        r#"name = "Projects"
default_type = "project"
folder_checksum = "blake3:not-real"

[[documents]]
slug = "kataan-redesign"
markdown = "kataan-redesign.md"
toml = "kataan-redesign.toml"
markdown_checksum = "blake3:not-real"
toml_checksum = "blake3:not-real"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::CHECKSUM_MISMATCH
            && diagnostic.path.as_deref() == Some("projects/index.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_ignores_prune_vendor_directories() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/opex/node_modules/undici/docs")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(
        root.join("projects/index.toml"),
        "type = \"project\"\nname = \"Projects\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    // A vendored file that would otherwise trip missing-folder-index.
    fs::write(
        root.join("projects/opex/node_modules/undici/docs/index.md"),
        "# vendor\n",
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(
        report.diagnostics.iter().all(|diagnostic| diagnostic
            .path
            .as_deref()
            .is_none_or(|path| !path.contains("node_modules"))),
        "node_modules must not produce diagnostics: {:?}",
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.path.clone())
            .collect::<Vec<_>>()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scan_config_ignore_patterns_are_honored() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/site/vendor/pkg")).unwrap();
    write_root_index(&root);
    // Same type folders as write_root_index, plus a custom `[scan]` ignore.
    fs::write(
        root.join(VAULT_CONFIG_FILE),
        r#"schema_version = "0.1.0"
name = "Test Vault"

[scan]
ignore = ["vendor"]

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#,
    )
    .unwrap();
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(
        root.join("projects/index.toml"),
        "type = \"project\"\nname = \"Projects\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    // `vendor` is not a default ignore; only the [scan] config prunes it.
    fs::write(root.join("projects/site/vendor/pkg/index.md"), "# vendor\n").unwrap();

    let report = validate(&root).unwrap();

    assert!(report.diagnostics.iter().all(|diagnostic| diagnostic
        .path
        .as_deref()
        .is_none_or(|path| !path.contains("vendor"))));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_folder_index_drift() {
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
markdown = "kataan-redesign.md"
"#,
    )
    .unwrap();
    fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();

    let report = validate(&root).unwrap();

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::INDEX_DRIFT));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_missing_type_definition_file() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("type")).unwrap();
    write_root_index(&root);
    fs::remove_file(root.join("type/project.md")).unwrap();
    fs::write(
        root.join("type/project.toml"),
        r#"type = "type-definition"
name = "project"
folder = "projects"
markdown = "project.md"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::MISSING_MARKDOWN_FILE
            && diagnostic.path.as_deref() == Some("type/project.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_missing_required_folder_but_allows_structural_type_folder() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects")).unwrap();
    write_root_index(&root);

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report.diagnostics.iter().any(|diagnostic| diagnostic.code
        == codes::MISSING_REQUIRED_FOLDER
        && diagnostic.path.as_deref() == Some("intake")));
    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/index.toml")));
    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/index.md")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_incomplete_nested_folder_index_pair() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/company-x/internal")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
    fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::MISSING_FOLDER_INDEX
            && diagnostic.path.as_deref() == Some("projects/company-x/index.toml")));
    assert!(report.diagnostics.iter().all(|diagnostic| !matches!(
        diagnostic.path.as_deref(),
        Some("projects/company-x/internal/index.md")
            | Some("projects/company-x/internal/index.toml")
    )));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_invalid_folder_index_toml_as_diagnostics() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/company-x")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(root.join("projects/index.toml"), "name = [\n").unwrap();
    fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
    fs::write(root.join("projects/company-x/index.toml"), "name = [\n").unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    for path in ["projects/index.toml", "projects/company-x/index.toml"] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == codes::INVALID_TOML
                    && diagnostic.path.as_deref() == Some(path)),
            "missing invalid TOML diagnostic for {path}: {:#?}",
            report.diagnostics
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validates_nested_documents_recursively() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/company-x")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
    fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
    fs::write(
        root.join("projects/company-x/index.toml"),
        "name = \"Company X\"\n",
    )
    .unwrap();
    fs::write(root.join("projects/company-x/orphan.md"), "# Orphan\n").unwrap();
    fs::write(root.join("projects/company-x/bad-status.md"), "# Bad\n").unwrap();
    fs::write(
        root.join("projects/company-x/bad-status.toml"),
        r#"type = "project"
status = "weird"
markdown = "bad-status.md"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::MISSING_TOML_SIDECAR
            && diagnostic.path.as_deref() == Some("projects/company-x/orphan.md")));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::INVALID_STATUS
            && diagnostic.path.as_deref() == Some("projects/company-x/bad-status.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_invalid_status_and_actor() {
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
        r#"
type = "project"
status = "Active"
markdown = "kataan-redesign.md"
created_by = "robot"
last_updated_by = "agent"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::INVALID_STATUS));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::INVALID_ACTOR));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_type_folder_mismatch() {
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
        r#"
type = "note"
markdown = "kataan-redesign.md"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::TYPE_FOLDER_MISMATCH
            && diagnostic.path.as_deref() == Some("projects/kataan-redesign.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_markdown_checksum_mismatch() {
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
        r#"
type = "project"
markdown = "kataan-redesign.md"
markdown_checksum = "blake3:not-the-real-hash"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::CHECKSUM_MISMATCH
            && diagnostic.path.as_deref() == Some("projects/kataan-redesign.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_markdown_metadata_path_mismatch() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects/company-x")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/index.md"), "# Projects\n").unwrap();
    fs::write(root.join("projects/index.toml"), "name = \"Projects\"\n").unwrap();
    fs::write(root.join("projects/overview.md"), "# Overview\n").unwrap();
    fs::write(
        root.join("projects/overview.toml"),
        "type = \"project\"\nmarkdown = \"other.md\"\n",
    )
    .unwrap();
    fs::write(root.join("projects/company-x/index.md"), "# Company\n").unwrap();
    fs::write(
        root.join("projects/company-x/index.toml"),
        "name = \"Company X\"\n",
    )
    .unwrap();
    fs::write(root.join("projects/company-x/deep.md"), "# Deep\n").unwrap();
    fs::write(
        root.join("projects/company-x/deep.toml"),
        "type = \"project\"\nmarkdown = \"other.md\"\n",
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    for path in ["projects/overview.toml", "projects/company-x/deep.toml"] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| diagnostic.code
                == codes::MARKDOWN_PATH_MISMATCH
                && diagnostic.path.as_deref() == Some(path)),
            "missing markdown path mismatch for {path}: {:#?}",
            report.diagnostics
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_missing_toml_sidecar() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects")).unwrap();
    write_root_index(&root);
    fs::write(
        root.join("projects/kataan-redesign.md"),
        "# Kataan Redesign\n",
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/kataan-redesign.md")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn treats_standalone_toml_as_file() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects")).unwrap();
    write_root_index(&root);
    fs::write(
        root.join("projects/kataan-redesign.toml"),
        r#"
type = "project"
markdown = "kataan-redesign.md"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.path.as_deref() != Some("projects/kataan-redesign.toml")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_edge_and_ontology_errors() {
    let root = unique_temp_dir();
    fs::create_dir_all(root.join("projects")).unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    write_root_index(&root);
    fs::write(root.join("projects/kataan-redesign.md"), "# Project\n").unwrap();
    fs::write(
        root.join("projects/kataan-redesign.toml"),
        r#"type = "project"
markdown = "kataan-redesign.md"

[edges]
derived_from = ["projects/kataan-redesign"]
missing_predicate = ["notes/summary"]
related_to = ["notes/missing"]
"#,
    )
    .unwrap();
    fs::write(root.join("notes/summary.md"), "# Summary\n").unwrap();
    fs::write(
        root.join("notes/summary.toml"),
        r#"type = "note"
markdown = "summary.md"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    for code in [
        codes::UNKNOWN_PREDICATE,
        codes::PREDICATE_TARGET_TYPE_MISMATCH,
        codes::UNRESOLVED_EDGE_TARGET,
    ] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "missing {code}: {:#?}",
            report.diagnostics
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validates_custom_type_folders() {
    let root = unique_temp_dir();
    crate::init::init_vault(&root, "Test Vault").unwrap();
    fs::create_dir_all(root.join("articles")).unwrap();
    let mut config = fs::read_to_string(root.join(VAULT_CONFIG_FILE)).unwrap();
    config.push_str("article = \"articles\"\n");
    fs::write(root.join(VAULT_CONFIG_FILE), config).unwrap();
    fs::write(root.join("articles/index.md"), "# Articles\n").unwrap();
    fs::write(
        root.join("articles/index.toml"),
        "type = \"article\"\nname = \"Articles\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    fs::write(root.join("articles/essay.md"), "# Essay\n").unwrap();
    fs::write(
        root.join("articles/essay.toml"),
        "type = \"article\"\nmarkdown = \"essay.md\"\n",
    )
    .unwrap();
    fs::write(root.join("type/article.md"), "# Article\n").unwrap();
    fs::write(
            root.join("type/article.toml"),
            "type = \"type-definition\"\nname = \"article\"\nfolder = \"articles\"\nicon = \"Newspaper\"\nmarkdown = \"article.md\"\n",
        )
        .unwrap();

    crate::rebuild::rebuild_indexes(&root).unwrap();
    let report = validate(&root).unwrap();

    assert!(report.is_ok(), "{:#?}", report.diagnostics);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_missing_type_definition_for_mapped_type() {
    let root = unique_temp_dir();
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(VAULT_CONFIG_FILE),
        r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
intake = "intake"
"#,
    )
    .unwrap();

    let report = validate(&root).unwrap();

    assert!(!report.is_ok());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == codes::MISSING_TYPE_FOLDER
            && diagnostic.message.contains("intake")));

    fs::remove_dir_all(root).unwrap();
}

fn write_root_index(root: &Path) {
    fs::write(
        root.join("ontology.toml"),
        r#"schema_version = "0.1.0"

[edges.related_to]
from = ["*"]
to = ["*"]
symmetric = true
cardinality = "many-to-many"

[edges.derived_from]
from = ["*"]
to = ["intake", "note"]
inverse = "derived"
cardinality = "many-to-many"
"#,
    )
    .unwrap();
    fs::write(
        root.join(VAULT_CONFIG_FILE),
        r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("type")).unwrap();
    fs::write(root.join("type/index.md"), "# Types\n").unwrap();
    fs::write(
        root.join("type/index.toml"),
        "type = \"type-definition\"\nname = \"Types\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    for (ty, folder) in [
        ("intake", "intake"),
        ("project", "projects"),
        ("person", "people"),
        ("note", "notes"),
        ("topic", "topics"),
        ("type-definition", "type"),
    ] {
        fs::write(
            root.join("type").join(format!("{ty}.md")),
            format!("# {ty}\n"),
        )
        .unwrap();
        fs::write(
                root.join("type").join(format!("{ty}.toml")),
                format!(
                    "type = \"type-definition\"\nname = \"{ty}\"\nfolder = \"{folder}\"\nmarkdown = \"{ty}.md\"\n"
                ),
            )
            .unwrap();
    }
}

fn unique_temp_dir() -> PathBuf {
    crate::test_support::unique_temp_dir("validate")
}

#[test]
fn rejects_bad_timestamps_with_distinct_codes() {
    let root = crate::test_support::unique_temp_dir("validate-timestamps");
    crate::init::init_vault(&root, "Test").unwrap();

    for (slug, value, expected) in [
        ("epoch", "1788013953", codes::UNIX_EPOCH_TIMESTAMP),
        ("zoneless", "2026-08-29T12:00:00", codes::ZONELESS_TIMESTAMP),
        ("garbage", "not_applicable", codes::INVALID_TIMESTAMP),
        ("impossible", "2026-02-30", codes::INVALID_TIMESTAMP),
    ] {
        fs::write(root.join(format!("notes/{slug}.md")), "# x\n").unwrap();
        fs::write(
            root.join(format!("notes/{slug}.toml")),
            format!("type = \"note\"\nmarkdown = \"{slug}.md\"\noccurred_at = \"{value}\"\n"),
        )
        .unwrap();
        // Rebuild first: an index-drift diagnostic would otherwise mask the
        // per-document checks we are exercising here.
        crate::rebuild::rebuild_indexes(&root).unwrap();

        let report = validate(&root).unwrap();
        assert!(
            report.diagnostics.iter().any(|d| d.code == expected),
            "`{value}` should report {expected}, got {:?}",
            report
                .diagnostics
                .iter()
                .map(|d| &d.code)
                .collect::<Vec<_>>()
        );
        fs::remove_file(root.join(format!("notes/{slug}.md"))).unwrap();
        fs::remove_file(root.join(format!("notes/{slug}.toml"))).unwrap();
    }

    // The same check must apply to nested documents, which a different walker
    // validates. Before these were factored into one helper, only half the
    // vault was covered.
    fs::create_dir_all(root.join("notes/deep")).unwrap();
    fs::write(root.join("notes/deep/index.md"), "# Deep\n").unwrap();
    fs::write(
        root.join("notes/deep/index.toml"),
        "name = \"Deep\"\ntype = \"note\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    fs::write(root.join("notes/deep/nested.md"), "# Nested\n").unwrap();
    fs::write(
        root.join("notes/deep/nested.toml"),
        "type = \"note\"\nmarkdown = \"nested.md\"\noccurred_at = \"1788013953\"\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();
    let report = validate(&root).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == codes::UNIX_EPOCH_TIMESTAMP
                && d.path.as_deref() == Some("notes/deep/nested.toml")),
        "nested document not checked: {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| (&d.code, &d.path))
            .collect::<Vec<_>>()
    );
    fs::remove_dir_all(root.join("notes/deep")).unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    // Every precision the vocabulary allows is accepted, unwidened.
    for value in ["2006", "2006-05", "2006-05-18", "2026-08-29T12:00:00Z"] {
        fs::write(root.join("notes/ok.md"), "# x\n").unwrap();
        fs::write(
            root.join("notes/ok.toml"),
            format!("type = \"note\"\nmarkdown = \"ok.md\"\noccurred_at = \"{value}\"\n"),
        )
        .unwrap();
        crate::rebuild::rebuild_indexes(&root).unwrap();
        assert!(
            validate(&root).unwrap().is_ok(),
            "`{value}` should validate"
        );
    }

    fs::remove_dir_all(root).unwrap();
}
