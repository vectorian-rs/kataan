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

/// A vault whose ontology constrains `person`, exercising each field type.
fn vault_with_node_schemas(name: &str) -> std::path::PathBuf {
    let root = crate::test_support::unique_temp_dir(name);
    crate::init::init_vault(&root, "Test").unwrap();
    let ontology = fs::read_to_string(root.join("ontology.toml")).unwrap();
    fs::write(
        root.join("ontology.toml"),
        format!(
            "{ontology}\n\
[nodes.person]\n\
required = [\"linkedin\"]\n\n\
[nodes.person.fields]\n\
linkedin = {{ type = \"string\" }}\n\
born = {{ type = \"date\" }}\n\
seen_at = {{ type = \"instant\" }}\n\
employment = {{ type = \"array\", items = \"interval\" }}\n\
mentor = {{ type = \"reference\", to = [\"person\"] }}\n"
        ),
    )
    .unwrap();
    root
}

fn write_person(root: &std::path::Path, slug: &str, extra: &str) {
    fs::write(root.join(format!("people/{slug}.md")), "# P\n").unwrap();
    fs::write(
        root.join(format!("people/{slug}.toml")),
        format!("type = \"person\"\nmarkdown = \"{slug}.md\"\n{extra}"),
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(root).unwrap();
}

fn codes_reported(root: &std::path::Path) -> Vec<String> {
    validate(root)
        .unwrap()
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect()
}

#[test]
fn node_schemas_enforce_required_fields_and_types() {
    let root = vault_with_node_schemas("schema-basics");

    // Missing a required field.
    write_person(&root, "nolink", "");
    assert!(codes_reported(&root).contains(&codes::MISSING_REQUIRED_FIELD.to_owned()));

    // Wrong primitive type.
    write_person(&root, "nolink", "linkedin = 42\n");
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    // A date field holding prose — the real shape this vault already contains.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nborn = \"pending approval\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_TIMESTAMP.to_owned()));

    // `instant` refuses a value that is only day-precise.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nseen_at = \"2026-08-29\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    // A well-formed document passes.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nborn = \"1979\"\nseen_at = \"2026-08-29T12:00:00Z\"\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn intervals_may_be_open_but_not_backwards() {
    let root = vault_with_node_schemas("schema-intervals");

    // An open interval is a fact about the world, not an error.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nfrom = \"2020-01-01\"\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    // Ending before it starts is.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nfrom = \"2020-01-01\"\nto = \"2019-01-01\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_INTERVAL.to_owned()));

    // So is a missing `from`.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nto = \"2019-01-01\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_INTERVAL.to_owned()));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn references_must_resolve_and_respect_their_target_types() {
    let root = vault_with_node_schemas("schema-references");
    write_person(
        &root,
        "mentee",
        "linkedin = \"x\"\nmentor = \"people/ghost\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::UNRESOLVED_FIELD_REFERENCE.to_owned()));

    // Pointing at a real document of the wrong type is also refused.
    fs::write(root.join("topics/rust.md"), "# R\n").unwrap();
    fs::write(
        root.join("topics/rust.toml"),
        "type = \"topic\"\nmarkdown = \"rust.md\"\n",
    )
    .unwrap();
    write_person(
        &root,
        "mentee",
        "linkedin = \"x\"\nmentor = \"topics/rust\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn undeclared_fields_stay_legal() {
    let root = vault_with_node_schemas("schema-open-world");

    // Schemas constrain what they declare. An unknown key must still pass, or
    // adding a schema would undo the unknown-key preservation in 86cb3a8.
    write_person(
        &root,
        "extra",
        "linkedin = \"x\"\nwebsite = \"example.com\"\ncompany_id = 7\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    // A type with no schema at all is entirely unconstrained.
    fs::write(root.join("notes/free.md"), "# N\n").unwrap();
    fs::write(
        root.join("notes/free.toml"),
        "type = \"note\"\nmarkdown = \"free.md\"\nanything = \"goes\"\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_escaping_type_folder_is_refused_everywhere() {
    let root = crate::test_support::unique_temp_dir("escape-type-folder");
    crate::init::init_vault(&root, "Test").unwrap();

    // A canary outside the vault: a cloned vault must not be able to touch it.
    let outside = root.parent().unwrap().join("escape-canary");
    fs::create_dir_all(&outside).unwrap();
    let canary = outside.join("index.toml");

    let config_path = root.join(crate::constants::VAULT_CONFIG_FILE);
    let text = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        text.replace("note = \"notes\"", "note = \"../escape-canary\""),
    )
    .unwrap();

    // validate names the problem rather than passing silently...
    let report = validate(&root).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == codes::UNSAFE_TYPE_FOLDER),
        "escape not reported: {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );

    // ...rebuild refuses outright rather than writing outside the vault...
    assert!(crate::rebuild::rebuild_indexes(&root).is_err());
    assert!(!canary.exists(), "rebuild created a file outside the vault");

    // ...and loading skips it rather than reading through it.
    let loaded = crate::vault::LoadedVault::load(&root).unwrap();
    assert!(
        loaded
            .documents
            .keys()
            .all(|id| !id.as_str().contains("escape-canary")),
        "load pulled documents from outside the vault"
    );

    fs::remove_dir_all(&outside).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_vault_that_cannot_load_is_reported_not_called_clean() {
    let root = crate::test_support::unique_temp_dir("unloadable");
    crate::init::init_vault(&root, "Test").unwrap();

    // An underscore is not a legal canonical id segment, so the walker returns
    // InvalidCanonicalIdAtPath and `Vault::load()` fails — meaning every CLI,
    // server and MCP read path errors out on this vault.
    fs::write(root.join("notes/my_note.md"), "# X\n").unwrap();
    fs::write(
        root.join("notes/my_note.toml"),
        "type = \"note\"\nmarkdown = \"my_note.md\"\n",
    )
    .unwrap();
    assert!(
        crate::vault::LoadedVault::load(&root).is_err(),
        "fixture no longer reproduces an unloadable vault"
    );

    // `validate` used to swallow that error and report the vault clean.
    let report = validate(&root).unwrap();
    assert!(
        !report.is_ok(),
        "validate called an unloadable vault clean: {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_inverse_colliding_with_another_predicate_is_reported() {
    use crate::ontology::{EdgePredicate, Ontology};
    use std::collections::BTreeMap;

    let predicate = |inverse: Option<&str>| EdgePredicate {
        from: vec!["*".to_owned()],
        to: vec!["*".to_owned()],
        inverse: inverse.map(str::to_owned),
        symmetric: false,
        cardinality: Some("many-to-many".to_owned()),
        description: None,
    };

    // A free-form inverse label needs no definition of its own — this is how
    // the default ontology works and must stay valid.
    let derived_label = Ontology {
        schema_version: "0.1.0".to_owned(),
        nodes: BTreeMap::new(),
        edges: BTreeMap::from([("owned_by".to_owned(), predicate(Some("owns")))]),
    };
    assert!(derived_label.validate().is_empty());

    // But if `owns` is itself a predicate pointing elsewhere, incoming edges
    // would be keyed ambiguously.
    let colliding = Ontology {
        schema_version: "0.1.0".to_owned(),
        nodes: BTreeMap::new(),
        edges: BTreeMap::from([
            ("owned_by".to_owned(), predicate(Some("owns"))),
            ("owns".to_owned(), predicate(Some("something_else"))),
        ]),
    };
    assert!(colliding
        .validate()
        .iter()
        .any(|d| d.code == codes::INVALID_ONTOLOGY_ENTRY));

    // A reciprocal pair is fine.
    let reciprocal = Ontology {
        schema_version: "0.1.0".to_owned(),
        nodes: BTreeMap::new(),
        edges: BTreeMap::from([
            ("owned_by".to_owned(), predicate(Some("owns"))),
            ("owns".to_owned(), predicate(Some("owned_by"))),
        ]),
    };
    assert!(reciprocal.validate().is_empty());
}
