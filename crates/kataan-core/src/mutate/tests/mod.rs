//! Write-path tests, split by what they write.
//!
//! The fixtures live here because both halves start from the same vault.

mod create;
mod edges;
mod on_disk;
mod update;

use super::*;

fn temp_vault(name: &str) -> std::path::PathBuf {
    let root = crate::test_support::unique_temp_dir(name);
    crate::init::init_vault(&root, "Test").unwrap();
    root
}

fn note(title: &str, body: &str) -> NewDocument {
    NewDocument {
        r#type: "note".to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
        ..Default::default()
    }
}

/// A sidecar carrying the three shapes an author can write that kataan does
/// not model: a custom scalar, a custom array, and a custom array-of-tables.
fn write_custom_keys(root: &std::path::Path, id: &CanonicalId) {
    let path = root.join(id.toml_path());
    let mut table = read_sidecar_table(&path).unwrap();
    table.insert(
        "linkedin".to_owned(),
        toml::Value::String("https://example.com/in/jane".to_owned()),
    );
    table.insert(
        "emails".to_owned(),
        string_array(vec!["jane@example.com".to_owned()]),
    );
    let mut employment = toml::Table::new();
    employment.insert(
        "from".to_owned(),
        toml::Value::String("2020-01-01".to_owned()),
    );
    table.insert(
        "employment".to_owned(),
        toml::Value::Array(vec![toml::Value::Table(employment)]),
    );
    write_sidecar_table(&path, &table).unwrap();
}

fn assert_custom_keys_intact(root: &std::path::Path, id: &CanonicalId) {
    let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    assert_eq!(
        table["linkedin"].as_str(),
        Some("https://example.com/in/jane"),
        "custom scalar was dropped"
    );
    assert_eq!(
        table["emails"].as_array().unwrap()[0].as_str(),
        Some("jane@example.com"),
        "custom array was dropped"
    );
    assert_eq!(
        table["employment"].as_array().unwrap()[0]["from"].as_str(),
        Some("2020-01-01"),
        "custom array-of-tables was dropped"
    );
}

/// Declare a `[nodes.note]` schema on a vault built by `temp_vault`.
fn with_note_schema(root: &std::path::Path, schema: &str) {
    let path = root.join("ontology.toml");
    let existing = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{existing}\n{schema}\n")).unwrap();
}

/// A source note and a target topic, linked by `related_to`.
fn linked_pair(name: &str) -> (std::path::PathBuf, CanonicalId, CanonicalId) {
    let root = temp_vault(name);
    let source = create_document(&root, note("Src", "a")).unwrap();
    let target = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("Tgt", "b")
        },
    )
    .unwrap();
    add_edge(&root, &source, "related_to", &target).unwrap();
    (root, source, target)
}

fn edge_targets(root: &std::path::Path, source: &CanonicalId, predicate: &str) -> Vec<String> {
    let sidecar = read_sidecar_table(&root.join(source.toml_path())).unwrap();
    sidecar
        .get("edges")
        .and_then(|edges| edges.get(predicate))
        .and_then(|targets| targets.as_array())
        .map(|targets| {
            targets
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
