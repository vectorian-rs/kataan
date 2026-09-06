//! Edge writes: adding, removing, and replacing a predicate's whole list.

use super::*;

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
fn remove_edge_deletes_the_edge_and_leaves_no_empty_table() {
    let (root, source, target) = linked_pair("remove-edge");
    assert_eq!(edge_targets(&root, &source, "related_to").len(), 1);

    remove_edge(&root, &source, "related_to", &target).unwrap();

    assert!(edge_targets(&root, &source, "related_to").is_empty());
    // An emptied predicate is dropped, so the file looks as it did before the
    // edge was ever added rather than carrying `related_to = []`.
    let sidecar = read_sidecar_table(&root.join(source.toml_path())).unwrap();
    assert!(sidecar.get("edges").is_none(), "{sidecar:?}");
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// Removing what is not there is not an error, and must not dirty the file:
/// the same rule `update_document` follows for a no-op patch.
#[test]
fn remove_edge_is_idempotent_and_does_not_move_updated_at() {
    let (root, source, target) = linked_pair("remove-edge-idempotent");
    remove_edge(&root, &source, "related_to", &target).unwrap();

    let path = root.join(source.toml_path());
    let stale = "2000-01-01T00:00:00Z";
    let mut sidecar = read_sidecar_table(&path).unwrap();
    sidecar.insert(
        "updated_at".to_owned(),
        toml::Value::String(stale.to_owned()),
    );
    write_sidecar_table(&path, &sidecar).unwrap();

    // Second removal, and one for a predicate the document never had.
    remove_edge(&root, &source, "related_to", &target).unwrap();
    remove_edge(&root, &source, "mentions", &target).unwrap();

    let after = read_sidecar_table(&path).unwrap();
    assert_eq!(
        after["updated_at"].as_str(),
        Some(stale),
        "a removal that changed nothing still rewrote the file"
    );

    std::fs::remove_dir_all(root).unwrap();
}

/// The point of removal is repair, so it must not require the edge to be legal.
/// Here the target document is deleted first, leaving an edge that `add_edge`
/// would refuse to create and whose target cannot be loaded at all.
#[test]
fn remove_edge_can_delete_an_edge_that_could_not_be_added() {
    let (root, source, target) = linked_pair("remove-edge-dangling");
    std::fs::remove_file(root.join(target.toml_path())).unwrap();
    std::fs::remove_file(root.join(target.markdown_path())).unwrap();

    // Precondition: this edge is now unaddable — the target is not there.
    assert!(add_edge(&root, &source, "related_to", &target).is_err());
    // But it is still removable.
    remove_edge(&root, &source, "related_to", &target).unwrap();
    assert!(edge_targets(&root, &source, "related_to").is_empty());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn replace_edges_for_predicate_sets_the_whole_list() {
    let (root, source, first) = linked_pair("replace-edges");
    let second = create_document(
        &root,
        NewDocument {
            r#type: "topic".to_owned(),
            ..note("Second", "c")
        },
    )
    .unwrap();

    // Correcting a wrong edge in one write: `first` out, `second` in.
    replace_edges_for_predicate(&root, &source, "related_to", std::slice::from_ref(&second))
        .unwrap();
    assert_eq!(
        edge_targets(&root, &source, "related_to"),
        vec![second.as_str().to_owned()]
    );
    assert!(!edge_targets(&root, &source, "related_to").contains(&first.as_str().to_owned()));

    // A repeat in the request is one edge: (source, predicate, target) is the
    // identity, so the list cannot carry a duplicate.
    replace_edges_for_predicate(
        &root,
        &source,
        "related_to",
        &[second.clone(), first.clone(), second.clone()],
    )
    .unwrap();
    assert_eq!(
        edge_targets(&root, &source, "related_to"),
        vec![second.as_str().to_owned(), first.as_str().to_owned()]
    );

    // Empty removes the predicate outright.
    replace_edges_for_predicate(&root, &source, "related_to", &[]).unwrap();
    assert!(edge_targets(&root, &source, "related_to").is_empty());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

/// Unlike removal, what gets written is validated — these are new edges.
#[test]
fn replace_edges_for_predicate_validates_incoming_targets() {
    let (root, source, target) = linked_pair("replace-edges-invalid");

    assert!(
        replace_edges_for_predicate(
            &root,
            &source,
            "no_such_predicate",
            std::slice::from_ref(&target)
        )
        .is_err(),
        "an undefined predicate must be refused"
    );
    // `subtopic_of` is declared `from = ["topic"]`, and the source is a note.
    // This has to fail on the endpoint type, not on the predicate being
    // unknown — otherwise the assertion proves nothing the line above did not.
    assert!(
        crate::ontology::Ontology::load(&root)
            .unwrap()
            .edges
            .contains_key("subtopic_of"),
        "the predicate must exist for this to test what it claims"
    );
    assert!(
        replace_edges_for_predicate(&root, &source, "subtopic_of", std::slice::from_ref(&target))
            .is_err(),
        "an endpoint type the ontology forbids must be refused"
    );
    // The refused calls left the existing edge alone.
    assert_eq!(edge_targets(&root, &source, "related_to").len(), 1);

    std::fs::remove_dir_all(root).unwrap();
}
