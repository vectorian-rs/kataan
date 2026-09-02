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

#[test]
fn create_document_produces_a_valid_document() {
    let root = temp_vault("create");

    let id = create_document(
        &root,
        NewDocument {
            status: Some("active".to_owned()),
            ..note("My First Note!", "# My First Note\n\nhello\n")
        },
    )
    .unwrap();

    assert_eq!(id.as_str(), "notes/my-first-note");
    assert!(root.join("notes/my-first-note.md").is_file());
    assert!(root.join("notes/my-first-note.toml").is_file());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn create_document_rejects_collision_and_unknown_type() {
    let root = temp_vault("collision");

    create_document(&root, note("Dup", "x")).unwrap();
    assert!(create_document(&root, note("Dup", "x")).is_err());
    assert!(create_document(
        &root,
        NewDocument {
            r#type: "nonsense".to_owned(),
            ..note("Y", "y")
        }
    )
    .is_err());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_document_changes_body_and_stays_valid() {
    let root = temp_vault("update");
    let id = create_document(&root, note("Note", "old body")).unwrap();

    update_document(
        &root,
        &id,
        Some("new body".to_owned()),
        DocumentPatch {
            status: Some("archived".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("notes/note.md")).unwrap(),
        "new body"
    );
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn add_edge_validates_against_the_ontology() {
    let root = temp_vault("edges");
    let source = create_document(&root, note("A", "a")).unwrap();
    let target = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("B", "b")
        },
    )
    .unwrap();

    // related_to is from=* to=*, so note -> topic is legal.
    add_edge(&root, &source, "related_to", &target).unwrap();
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    // subtopic_of requires a topic source; a note source is rejected.
    assert!(add_edge(&root, &source, "subtopic_of", &target).is_err());
    // An unknown predicate is rejected.
    assert!(add_edge(&root, &source, "bogus", &target).is_err());

    std::fs::remove_dir_all(root).unwrap();
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

#[test]
fn update_document_preserves_unknown_sidecar_keys() {
    let root = temp_vault("preserve-update");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);

    update_document(
        &root,
        &id,
        Some("changed".to_owned()),
        DocumentPatch {
            status: Some("active".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_custom_keys_intact(&root, &id);
    let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    assert_eq!(table["status"].as_str(), Some("active"));
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn add_edge_preserves_sibling_keys() {
    let root = temp_vault("preserve-edge");
    let source = create_document(&root, note("Jane", "a")).unwrap();
    let target = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("B", "b")
        },
    )
    .unwrap();
    write_custom_keys(&root, &source);

    add_edge(&root, &source, "related_to", &target).unwrap();

    assert_custom_keys_intact(&root, &source);
    let table = read_sidecar_table(&root.join(source.toml_path())).unwrap();
    assert_eq!(
        table["edges"]["related_to"].as_array().unwrap()[0].as_str(),
        Some("topics/b")
    );
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_update_touches_only_the_keys_it_changes() {
    let root = temp_vault("minimal-diff");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);
    let path = root.join(id.toml_path());
    let before = std::fs::read_to_string(&path).unwrap();

    update_document(&root, &id, None, DocumentPatch::default()).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();

    // `last_updated_by` is already `agent`, so a no-op patch must leave the
    // file byte-identical — key order included.
    assert_eq!(before, after);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn create_document_writes_and_rejects_extra_fields() {
    let root = temp_vault("create-extra");

    let id = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([(
                "linkedin".to_owned(),
                toml::Value::String("https://example.com/in/jane".to_owned()),
            )]),
            ..note("Jane", "hello")
        },
    )
    .unwrap();

    let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    assert_eq!(
        table["linkedin"].as_str(),
        Some("https://example.com/in/jane")
    );
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    // A reserved key would serialize twice and produce invalid TOML.
    let reserved = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([("type".to_owned(), toml::Value::String("person".to_owned()))]),
            ..note("Reserved", "x")
        },
    );
    assert!(reserved.is_err());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_keys_are_readable_through_document_metadata() {
    let root = temp_vault("expose-extra");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);

    let record = Vault::open(&root)
        .unwrap()
        .load_document_record(&id)
        .unwrap();

    assert_eq!(
        record.metadata.extra["linkedin"].as_str(),
        Some("https://example.com/in/jane")
    );
    assert!(record.metadata.extra.contains_key("employment"));
    // Keys kataan models must not leak into `extra`.
    for reserved in RESERVED_KEYS {
        assert!(
            !record.metadata.extra.contains_key(*reserved),
            "`{reserved}` leaked into extra"
        );
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rebuild_indexes_is_idempotent_over_custom_keys() {
    let root = temp_vault("rebuild-extra");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);

    crate::rebuild::rebuild_indexes(&root).unwrap();
    let once = std::fs::read_to_string(root.join(id.toml_path())).unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();
    let twice = std::fs::read_to_string(root.join(id.toml_path())).unwrap();

    assert_eq!(once, twice);
    assert_custom_keys_intact(&root, &id);
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn writes_stamp_transaction_time_in_iso8601() {
    let root = temp_vault("time-stamps");
    let id = create_document(&root, note("Stamped", "before")).unwrap();

    let created = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    let created_at = created["created_at"].as_str().unwrap().to_owned();
    assert_eq!(created["updated_at"].as_str(), Some(created_at.as_str()));
    // ISO-8601 UTC, never the bare epoch the old helper produced.
    assert!(
        crate::time::Timestamp::parse(&created_at).is_ok(),
        "{created_at}"
    );
    assert!(created_at.ends_with('Z'), "{created_at}");

    update_document(
        &root,
        &id,
        Some("after".to_owned()),
        DocumentPatch::default(),
    )
    .unwrap();
    let updated = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    // created_at is immutable; updated_at moves.
    assert_eq!(updated["created_at"].as_str(), Some(created_at.as_str()));
    assert!(crate::time::Timestamp::parse(updated["updated_at"].as_str().unwrap()).is_ok());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resending_identical_content_is_not_a_change() {
    let root = temp_vault("time-noop-body");
    let id = create_document(&root, note("Same", "identical")).unwrap();
    let path = root.join(id.toml_path());
    let before = std::fs::read_to_string(&path).unwrap();

    // A caller resending the body it already has must not move updated_at.
    update_document(
        &root,
        &id,
        Some("identical".to_owned()),
        DocumentPatch::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_legacy_epoch_in_kataan_toml_loads_then_heals_on_rebuild() {
    let root = temp_vault("time-migration");
    // Recreate the pre-1.0 shape: `updated_at` written as a bare epoch.
    let config_path = root.join(crate::constants::VAULT_CONFIG_FILE);
    let legacy = std::fs::read_to_string(&config_path).unwrap().replace(
        &format!("updated_at = \"{}\"", crate::time::iso8601_utc_now()),
        "",
    );
    let mut table: toml::Table = legacy.parse().unwrap();
    table.insert(
        "updated_at".to_owned(),
        toml::Value::String("1788013953".to_owned()),
    );
    write_sidecar_table(&config_path, &table).unwrap();

    // Lenient read: the vault still loads and validates with the old value.
    assert!(Vault::open(&root).is_ok());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    // Strict write: the next rebuild replaces it with ISO-8601.
    rebuild::rebuild_indexes(&root).unwrap();
    let healed = read_sidecar_table(&config_path).unwrap();
    let value = healed["updated_at"].as_str().unwrap();
    assert!(
        crate::time::Timestamp::parse(value).is_ok(),
        "epoch was not healed: {value}"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn parent_cannot_place_a_document_outside_its_type_folder() {
    let root = temp_vault("parent-type");

    // An unregistered type is refused whether or not a parent is given.
    assert!(create_document(
        &root,
        NewDocument {
            r#type: "not-a-type".to_owned(),
            parent: Some("notes".to_owned()),
            ..note("X", "x")
        }
    )
    .is_err());

    // A parent belonging to a different type would produce a document that
    // `validate` then reports as a type-folder mismatch.
    assert!(create_document(
        &root,
        NewDocument {
            parent: Some("people".to_owned()),
            ..note("Y", "y")
        }
    )
    .is_err());

    // A subfolder of the type's own folder is fine.
    let id = create_document(
        &root,
        NewDocument {
            parent: Some("notes".to_owned()),
            ..note("Z", "z")
        },
    )
    .unwrap();
    assert_eq!(id.as_str(), "notes/z");
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn add_edge_records_when_it_changed_the_document() {
    let root = temp_vault("edge-provenance");
    let source = create_document(&root, note("Src", "a")).unwrap();
    let target = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("Tgt", "b")
        },
    )
    .unwrap();
    // Back-date the stamp so the assertion cannot depend on wall-clock
    // granularity: `iso8601_utc_now` is second-resolution, so a create and
    // an edge write in the same second produce the same string.
    let path = root.join(source.toml_path());
    let stale = "2000-01-01T00:00:00Z";
    let mut before = read_sidecar_table(&path).unwrap();
    before.insert(
        "updated_at".to_owned(),
        toml::Value::String(stale.to_owned()),
    );
    write_sidecar_table(&path, &before).unwrap();

    add_edge(&root, &source, "related_to", &target).unwrap();

    let after = read_sidecar_table(&path).unwrap();
    assert_ne!(
        after["updated_at"].as_str(),
        Some(stale),
        "an edge write left updated_at pointing at an unrelated change"
    );
    assert!(crate::time::Timestamp::parse(after["updated_at"].as_str().unwrap()).is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn create_document_can_place_a_scope_typed_document() {
    let root = temp_vault("create-scoped");
    // A type placed by patterns and by a folder scope, exactly as the deck
    // migration does it: no `kataan.toml [type_folders]` entry at all.
    std::fs::write(root.join("type/deck.md"), "# Deck\n").unwrap();
    std::fs::write(
        root.join("type/deck.toml"),
        r#"type = "type-definition"
name = "deck"
extends = "project"
folders = ["projects/*/decks"]
markdown = "deck.md"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("projects/acme/decks")).unwrap();
    std::fs::write(root.join("projects/acme/decks/index.md"), "# Decks\n").unwrap();
    std::fs::write(
        root.join("projects/acme/decks/index.toml"),
        "type = \"project\"\nmarkdown = \"index.md\"\nname = \"Decks\"\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();

    let id = create_document(
        &root,
        NewDocument {
            r#type: "deck".to_owned(),
            title: "Launch".to_owned(),
            body: "# Launch\n".to_owned(),
            parent: Some("projects/acme/decks".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(id.as_str(), "projects/acme/decks/launch");
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// Declare a `[nodes.note]` schema on a vault built by `temp_vault`.
fn with_note_schema(root: &std::path::Path, schema: &str) {
    let path = root.join("ontology.toml");
    let existing = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{existing}\n{schema}\n")).unwrap();
}

/// This module's stated invariant is that `validate` never has to report
/// something kataan itself wrote. Before this, `[nodes.*]` schemas were checked
/// only on the next `validate` run — the value was already on disk.
#[test]
fn create_document_refuses_what_validate_would_reject() {
    let root = temp_vault("create-schema");
    with_note_schema(
        &root,
        r#"
[nodes.note]
required = ["source_url"]

[nodes.note.fields]
source_url = { type = "string" }
reviewed_on = { type = "date" }
"#,
    );

    // A required field the caller did not supply.
    let missing = create_document(&root, note("No Source", "body"));
    let message = missing.unwrap_err().to_string();
    assert!(message.contains("source_url"), "{message}");

    // A field whose value is the wrong type.
    let wrong_type = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([("source_url".to_owned(), toml::Value::Integer(7))]),
            ..note("Numeric Source", "body")
        },
    );
    assert!(wrong_type.is_err(), "an integer is not a string");

    // A date field that is not RFC 3339.
    let bad_date = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([
                ("source_url".to_owned(), toml::Value::String("x".to_owned())),
                (
                    "reviewed_on".to_owned(),
                    toml::Value::String("2026".to_owned()),
                ),
            ]),
            ..note("Bad Date", "body")
        },
    );
    assert!(bad_date.is_err(), "`2026` is not RFC 3339");

    // Satisfying the schema still writes, and the result validates.
    let id = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([
                (
                    "source_url".to_owned(),
                    toml::Value::String("https://x".to_owned()),
                ),
                (
                    "reviewed_on".to_owned(),
                    toml::Value::String("2026-08-29".to_owned()),
                ),
            ]),
            ..note("Good", "body")
        },
    )
    .unwrap();
    assert_eq!(id.as_str(), "notes/good");
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// `occurred_at` was checked for syntax but never against a schema declaring
/// `instant`, so a bare day was accepted at write and reported later.
#[test]
fn create_document_enforces_declared_timestamp_precision() {
    let root = temp_vault("create-precision");
    with_note_schema(
        &root,
        r#"
[nodes.note.fields]
occurred_at = { type = "instant" }
"#,
    );

    let day = create_document(
        &root,
        NewDocument {
            occurred_at: Some("2026-08-29".to_owned()),
            ..note("Day Only", "body")
        },
    );
    assert!(day.is_err(), "a full-date does not satisfy `instant`");

    create_document(
        &root,
        NewDocument {
            occurred_at: Some("2026-08-29T12:00:00Z".to_owned()),
            ..note("An Instant", "body")
        },
    )
    .unwrap();

    std::fs::remove_dir_all(root).unwrap();
}

/// An update must be held to the same rule, including for keys it did not
/// touch: a schema can require a field that an unrelated edit leaves missing.
#[test]
fn update_document_refuses_what_validate_would_reject() {
    let root = temp_vault("update-schema");
    let id = create_document(&root, note("Subject", "body")).unwrap();
    with_note_schema(
        &root,
        r#"
[nodes.note.fields]
occurred_at = { type = "instant" }
"#,
    );

    let bad = update_document(
        &root,
        &id,
        None,
        DocumentPatch {
            occurred_at: Some("2026-08-29".to_owned()),
            ..Default::default()
        },
    );
    assert!(bad.is_err(), "a full-date does not satisfy `instant`");

    update_document(
        &root,
        &id,
        None,
        DocumentPatch {
            occurred_at: Some("2026-08-29T12:00:00Z".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}
