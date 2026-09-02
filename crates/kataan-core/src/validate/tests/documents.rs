use super::*;

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

#[test]
fn native_toml_dates_are_reported_wherever_they_appear() {
    let root = crate::test_support::unique_temp_dir("native-dates");
    crate::init::init_vault(&root, "Test").unwrap();

    fs::write(root.join("notes/dated.md"), "# D\n").unwrap();
    fs::write(
        root.join("notes/dated.toml"),
        // Unquoted: these are native TOML date values, not strings. Nested and
        // in an array too, since a date one level down is the same problem.
        "type = \"note\"\nmarkdown = \"dated.md\"\n\
         signed_on = 2024-01-02\n\
         seen = [2024-03-04]\n\n\
         [rate_card]\n\
         effective = 2024-05-06\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let report = validate(&root).unwrap();
    let dated: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == codes::NATIVE_TOML_DATETIME)
        .map(|d| d.message.as_str())
        .collect();

    assert_eq!(
        dated.len(),
        3,
        "expected top-level, array and nested: {dated:?}"
    );
    assert!(dated.iter().any(|m| m.contains("`signed_on`")), "{dated:?}");
    assert!(dated.iter().any(|m| m.contains("`seen[0]`")), "{dated:?}");
    assert!(
        dated.iter().any(|m| m.contains("`rate_card.effective`")),
        "{dated:?}"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn quoted_dates_pass_at_every_precision() {
    let root = crate::test_support::unique_temp_dir("quoted-dates");
    crate::init::init_vault(&root, "Test").unwrap();

    // The whole point of quoting: TOML cannot express "2019, month unknown",
    // so a native date would force a day nobody knows.
    fs::write(root.join("notes/ok.md"), "# D\n").unwrap();
    fs::write(
        root.join("notes/ok.toml"),
        "type = \"note\"\nmarkdown = \"ok.md\"\n\
         joined = \"2019\"\n\
         left = \"2019-05\"\n\
         signed_on = \"2024-01-02\"\n\
         seen_at = \"2024-01-02T09:30:00Z\"\n",
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
