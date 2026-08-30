use super::*;

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
