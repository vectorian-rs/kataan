//! `update_document`: applying a patch, and what a refused patch must leave
//! exactly as it was.

use super::*;

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
fn update_document_preserves_comments_and_formatting() {
    let root = temp_vault("preserve-comments");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    let sidecar = root.join(id.toml_path());

    // An author edits the sidecar by hand, as a filesystem-native format
    // invites. Everything here is invisible to a `toml::Value` round trip.
    let original = std::fs::read_to_string(&sidecar).unwrap();
    std::fs::write(
        &sidecar,
        format!("# Jane is the point of contact.\n{original}\nlabels = [\"alpha\", \"beta\"]\n"),
    )
    .unwrap();

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

    let after = std::fs::read_to_string(&sidecar).unwrap();
    assert!(
        after.contains("# Jane is the point of contact."),
        "comment lost on a metadata write: {after}"
    );
    // Untouched by the patch, so not re-rendered — an exploded array here would
    // mean every save reformats the file around the one key that changed.
    assert!(
        after.contains("labels = [\"alpha\", \"beta\"]"),
        "inline array reflowed: {after}"
    );
    assert!(after.contains("status = \"active\""), "{after}");
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

/// A rejected update must leave *both* files untouched.
///
/// The body was written before the schema check, so a patch that changed the
/// body and violated the type's schema returned an error with the new body
/// already on disk — the caller told the write was rejected, half of it landed.
#[test]
fn a_rejected_update_leaves_the_body_untouched() {
    let root = temp_vault("update-atomicity");
    let id = create_document(&root, note("Subject", "original body")).unwrap();
    with_note_schema(
        &root,
        r#"
[nodes.note.fields]
occurred_at = { type = "instant" }
"#,
    );

    let markdown = root.join(id.markdown_path());
    let sidecar = root.join(id.toml_path());
    let markdown_before = std::fs::read_to_string(&markdown).unwrap();
    let sidecar_before = std::fs::read_to_string(&sidecar).unwrap();

    // Changes the body *and* violates the schema: a full-date does not satisfy
    // `instant`.
    let rejected = update_document(
        &root,
        &id,
        Some("replacement body".to_owned()),
        DocumentPatch {
            occurred_at: Some("2026-08-29".to_owned()),
            ..Default::default()
        },
    );
    assert!(rejected.is_err(), "the patch violates the schema");

    assert_eq!(
        std::fs::read_to_string(&markdown).unwrap(),
        markdown_before,
        "the body was written despite the patch being rejected"
    );
    assert_eq!(
        std::fs::read_to_string(&sidecar).unwrap(),
        sidecar_before,
        "the sidecar changed despite the patch being rejected"
    );

    // The same patch with a valid timestamp still lands, body included.
    update_document(
        &root,
        &id,
        Some("replacement body".to_owned()),
        DocumentPatch {
            occurred_at: Some("2026-08-29T12:00:00Z".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&markdown).unwrap(),
        "replacement body"
    );
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// Custom fields were write-once: `create_document` took them and nothing could
/// change them afterwards, so any consumer modelling data in a custom key — the
/// thing `[nodes.*]` schemas exist to describe — had a vault it could not edit.

#[test]
fn update_document_sets_and_removes_custom_fields() {
    let root = temp_vault("patch-fields");
    let id = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([
                (
                    "source_url".to_owned(),
                    toml::Value::String("https://old".to_owned()),
                ),
                (
                    "keep_me".to_owned(),
                    toml::Value::String("untouched".to_owned()),
                ),
            ]),
            ..note("Fielded", "body")
        },
    )
    .unwrap();

    let read = |root: &std::path::Path| read_sidecar_table(&root.join(id.toml_path())).unwrap();

    // Set one, add one, remove one, and say nothing about the fourth.
    update_document(
        &root,
        &id,
        None,
        DocumentPatch {
            fields: BTreeMap::from([
                (
                    "source_url".to_owned(),
                    Some(toml::Value::String("https://new".to_owned())),
                ),
                ("added".to_owned(), Some(toml::Value::Integer(7))),
                ("keep_me".to_owned(), None),
            ]),
            ..Default::default()
        },
    )
    .unwrap();

    let sidecar = read(&root);
    assert_eq!(sidecar["source_url"].as_str(), Some("https://new"));
    assert_eq!(sidecar["added"].as_integer(), Some(7));
    assert!(
        !sidecar.contains_key("keep_me"),
        "a null did not remove the key"
    );
    // Untouched keys survive, as they do for every other patch field.
    assert_eq!(sidecar["type"].as_str(), Some("note"));

    // Reserved keys are refused here as they are on create — writing one would
    // emit it twice, since it also has its own patch field.
    let reserved = update_document(
        &root,
        &id,
        None,
        DocumentPatch {
            fields: BTreeMap::from([(
                "status".to_owned(),
                Some(toml::Value::String("active".to_owned())),
            )]),
            ..Default::default()
        },
    );
    assert!(reserved.is_err(), "`status` has its own patch field");

    assert!(crate::validate::validate(&root).unwrap().is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

/// A field removal is a change, so it must move `updated_at` and not be
/// mistaken for a no-op.

#[test]
fn removing_a_field_is_not_a_no_op() {
    let root = temp_vault("patch-fields-noop");
    let id = create_document(
        &root,
        NewDocument {
            extra: BTreeMap::from([("doomed".to_owned(), toml::Value::String("x".to_owned()))]),
            ..note("Doomed", "body")
        },
    )
    .unwrap();
    let path = root.join(id.toml_path());
    let stale = "2000-01-01T00:00:00Z";
    let mut sidecar = read_sidecar_table(&path).unwrap();
    sidecar.insert(
        "updated_at".to_owned(),
        toml::Value::String(stale.to_owned()),
    );
    write_sidecar_table(&path, &sidecar).unwrap();

    update_document(
        &root,
        &id,
        None,
        DocumentPatch {
            fields: BTreeMap::from([("doomed".to_owned(), None)]),
            ..Default::default()
        },
    )
    .unwrap();

    let after = read_sidecar_table(&path).unwrap();
    assert!(!after.contains_key("doomed"));
    assert_ne!(
        after["updated_at"].as_str(),
        Some(stale),
        "a removal left updated_at pointing at an unrelated change"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn two_saves_sharing_one_token_cannot_both_win() {
    // The precondition's whole job. Both writes land inside the same second, so
    // a second-resolution stamp left the token unchanged after the first —
    // and the second write, still holding the original token, matched and
    // overwrote it. Confirmed over HTTP before the stamp was made monotonic:
    // two PATCHes, both 200, first edit gone.
    let root = temp_vault("same-second-conflict");
    let id = create_document(&root, note("Subject", "original")).unwrap();

    // Give it a stamp to hold.
    update_document(
        &root,
        &id,
        Some("seed".to_owned()),
        DocumentPatch::default(),
    )
    .unwrap();
    let token = read_sidecar_table(&root.join(id.toml_path())).unwrap()["updated_at"]
        .as_str()
        .unwrap()
        .to_owned();

    update_document(
        &root,
        &id,
        Some("first".to_owned()),
        DocumentPatch {
            expected_updated_at: Some(token.clone()),
            ..Default::default()
        },
    )
    .expect("the first write holds the current token");

    let second = update_document(
        &root,
        &id,
        Some("second".to_owned()),
        DocumentPatch {
            expected_updated_at: Some(token),
            ..Default::default()
        },
    );

    assert!(
        matches!(second, Err(crate::Error::Conflict(_))),
        "the second write held a stale token and must be refused: {second:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(id.markdown_path())).unwrap(),
        "first",
        "the refused write must not have replaced the body"
    );

    std::fs::remove_dir_all(root).unwrap();
}
