//! `create_document`: where a new document lands, and what the vault refuses
//! to create in the first place.

use super::*;

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

/// Nested constraints must hold at the write boundary too, not only in
/// `validate` — the whole point of enforcing schemas on write is that nothing
/// invalid reaches disk.
#[test]
fn create_document_enforces_nested_table_fields() {
    let root = temp_vault("create-nested");
    with_note_schema(
        &root,
        r#"
[nodes.note.fields.rate_card]
type = "table"
required = ["currency"]

[nodes.note.fields.rate_card.fields]
currency = { type = "string" }
effective_date = { type = "date" }
"#,
    );

    let bad_date = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([(
                "rate_card".to_owned(),
                toml::Value::Table(toml::Table::from_iter([
                    ("currency".to_owned(), toml::Value::String("EUR".to_owned())),
                    (
                        "effective_date".to_owned(),
                        toml::Value::String("pending approval".to_owned()),
                    ),
                ])),
            )]),
            ..note("Bad Nested", "body")
        },
    );
    assert!(bad_date.is_err(), "a date inside a table must be checked");

    let missing_required = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([(
                "rate_card".to_owned(),
                toml::Value::Table(toml::Table::from_iter([(
                    "effective_date".to_owned(),
                    toml::Value::String("2026-08-29".to_owned()),
                )])),
            )]),
            ..note("Missing Nested", "body")
        },
    );
    assert!(
        missing_required.is_err(),
        "`currency` is required inside the table"
    );

    create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([(
                "rate_card".to_owned(),
                toml::Value::Table(toml::Table::from_iter([
                    ("currency".to_owned(), toml::Value::String("EUR".to_owned())),
                    (
                        "effective_date".to_owned(),
                        toml::Value::String("2026-08-29".to_owned()),
                    ),
                ])),
            )]),
            ..note("Good Nested", "body")
        },
    )
    .unwrap();
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// A reference nested inside a table must resolve against the document index.
///
/// `enforce_document_schema` loads that index only when the type declares a
/// reference, and the predicate deciding that looked only at top-level fields.
/// So a type whose only reference lived at `rate_card.approved_by` skipped the
/// load, `known_document_types` stayed empty, and every target it named was
/// reported as not existing — making nested references impossible to satisfy.

#[test]
fn a_nested_reference_resolves_against_documents_that_exist() {
    let root = temp_vault("create-nested-reference");
    with_note_schema(
        &root,
        r#"
[nodes.note.fields.rate_card]
type = "table"

[nodes.note.fields.rate_card.fields]
approved_by = { type = "reference", to = ["topic"] }
"#,
    );

    let target = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("Approver", "x")
        },
    )
    .unwrap();

    let card = |id: &str| {
        BTreeMap::from([(
            "rate_card".to_owned(),
            toml::Value::Table(toml::Table::from_iter([(
                "approved_by".to_owned(),
                toml::Value::String(id.to_owned()),
            )])),
        )])
    };

    // The target exists, so the write is accepted.
    create_document(
        &root,
        NewDocument {
            extra: card(target.as_str()),
            ..note("Good Ref", "x")
        },
    )
    .expect("a nested reference to an existing document must be accepted");

    // And a target that does not exist is still refused, so the fix did not
    // simply stop checking.
    let missing = create_document(
        &root,
        NewDocument {
            extra: card("topics/ghost"),
            ..note("Bad Ref", "x")
        },
    );
    assert!(
        missing.is_err(),
        "a dangling nested reference must be refused"
    );

    assert!(crate::validate::validate(&root).unwrap().is_ok());
    std::fs::remove_dir_all(root).unwrap();
}
