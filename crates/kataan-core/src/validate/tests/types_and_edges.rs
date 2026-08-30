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
