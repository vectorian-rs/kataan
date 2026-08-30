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
