use serde_json::json;

use super::*;
use crate::mutate::{self, NewDocument};

/// A vault with two topics and a note, wired with one symmetric edge
/// (`related_to`) and one inverse-backed edge (`subtopic_of`/`has_subtopic`).
fn vault_with_edges(name: &str) -> std::path::PathBuf {
    let root = crate::test_support::unique_temp_dir(name);
    crate::init::init_vault(&root, "Test").unwrap();

    for (ty, title) in [
        ("topic", "Rust"),
        ("topic", "Systems"),
        ("note", "Field Notes"),
    ] {
        mutate::create_document(
            &root,
            NewDocument {
                r#type: ty.to_owned(),
                title: title.to_owned(),
                body: title.to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    let rust = CanonicalId::parse("topics/rust").unwrap();
    let systems = CanonicalId::parse("topics/systems").unwrap();
    let note = CanonicalId::parse("notes/field-notes").unwrap();

    mutate::add_edge(&root, &rust, "subtopic_of", &systems).unwrap();
    mutate::add_edge(&root, &note, "related_to", &rust).unwrap();
    root
}

#[test]
fn incoming_edges_answer_what_outgoing_cannot() {
    let root = vault_with_edges("neighbors-incoming");
    let vault = LoadedVault::load(&root).unwrap();
    let systems = CanonicalId::parse("topics/systems").unwrap();

    // `topics/systems` declares no edges at all — its sidecar is empty. The
    // relationship exists only as `topics/rust subtopic_of topics/systems`,
    // so this is exactly the query `get_document` cannot answer.
    let result = neighbors(&vault, &systems, None, Direction::Both).unwrap();

    assert!(result.out.is_empty(), "systems declares no outgoing edges");
    let children = &result.r#in["has_subtopic"];
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, "topics/rust");
    // Hydrated, so a caller can render the link without a second fetch.
    assert_eq!(children[0].r#type, "topic");
    assert_eq!(children[0].title, "Rust");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn direction_and_predicate_filters_narrow_the_result() {
    let root = vault_with_edges("neighbors-filters");
    let vault = LoadedVault::load(&root).unwrap();
    let rust = CanonicalId::parse("topics/rust").unwrap();

    let out_only = neighbors(&vault, &rust, None, Direction::Out).unwrap();
    assert!(out_only.r#in.is_empty());
    assert!(out_only.out.contains_key("subtopic_of"));

    let in_only = neighbors(&vault, &rust, None, Direction::In).unwrap();
    assert!(in_only.out.is_empty());

    let one = neighbors(&vault, &rust, Some("subtopic_of"), Direction::Both).unwrap();
    assert_eq!(one.out.keys().collect::<Vec<_>>(), ["subtopic_of"]);

    assert!(neighbors(&vault, &rust, Some("nope"), Direction::Both)
        .unwrap()
        .out
        .is_empty());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_symmetric_edge_is_reachable_from_both_sides_but_exported_once() {
    let root = vault_with_edges("subgraph-symmetric");
    let vault = LoadedVault::load(&root).unwrap();
    let rust = CanonicalId::parse("topics/rust").unwrap();
    let note = CanonicalId::parse("notes/field-notes").unwrap();

    // `related_to` is symmetric, so traversal works from either endpoint...
    assert_eq!(
        neighbors(&vault, &rust, Some("related_to"), Direction::Both)
            .unwrap()
            .out["related_to"][0]
            .id,
        "notes/field-notes"
    );
    assert_eq!(
        neighbors(&vault, &note, Some("related_to"), Direction::Both)
            .unwrap()
            .out["related_to"][0]
            .id,
        "topics/rust"
    );

    // ...but the export contains it once, in the authored direction. Iterating
    // the direction indexes instead would emit it twice.
    let graph = subgraph(&vault, &[], &[]);
    let related: Vec<_> = graph
        .links
        .iter()
        .filter(|link| link.predicate == "related_to")
        .collect();
    assert_eq!(
        related.len(),
        1,
        "symmetric edge double-counted: {related:?}"
    );
    assert_eq!(related[0].source, "notes/field-notes");
    assert_eq!(related[0].target, "topics/rust");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn inverse_edges_are_not_exported_as_extra_links() {
    let root = vault_with_edges("subgraph-inverse");
    let vault = LoadedVault::load(&root).unwrap();

    let graph = subgraph(&vault, &[], &[]);

    // `subtopic_of` has inverse `has_subtopic`. Only the authored direction is
    // a link; the inverse exists for traversal, not for export.
    assert_eq!(
        graph
            .links
            .iter()
            .filter(|link| link.predicate == "subtopic_of")
            .count(),
        1
    );
    assert!(
        !graph
            .links
            .iter()
            .any(|link| link.predicate == "has_subtopic"),
        "derived inverse leaked into the export"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn filters_keep_the_result_internally_consistent() {
    let root = vault_with_edges("subgraph-filters");
    let vault = LoadedVault::load(&root).unwrap();

    let topics_only = subgraph(&vault, &["topic".to_owned()], &[]);
    assert!(topics_only.nodes.iter().all(|node| node.r#type == "topic"));
    // The note->topic `related_to` link must be dropped: its source is gone.
    let ids: BTreeSet<&str> = topics_only.nodes.iter().map(|n| n.id.as_str()).collect();
    for link in &topics_only.links {
        assert!(
            ids.contains(link.source.as_str()) && ids.contains(link.target.as_str()),
            "link {link:?} dangles outside the filtered node set"
        );
    }
    assert!(!topics_only
        .links
        .iter()
        .any(|link| link.predicate == "related_to"));

    let by_predicate = subgraph(&vault, &[], &["subtopic_of".to_owned()]);
    assert!(by_predicate
        .links
        .iter()
        .all(|link| link.predicate == "subtopic_of"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn export_is_deterministic_across_rebuilds() {
    let root = vault_with_edges("subgraph-deterministic");

    let first =
        serde_json::to_value(subgraph(&LoadedVault::load(&root).unwrap(), &[], &[])).unwrap();
    let second =
        serde_json::to_value(subgraph(&LoadedVault::load(&root).unwrap(), &[], &[])).unwrap();

    assert_eq!(first, second, "graph export is not reproducible");
    assert_ne!(first["nodes"], json!([]));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn neighbors_of_an_unknown_document_errors() {
    let root = vault_with_edges("neighbors-unknown");
    let vault = LoadedVault::load(&root).unwrap();
    let missing = CanonicalId::parse("topics/nope").unwrap();

    assert!(neighbors(&vault, &missing, None, Direction::Both).is_err());

    std::fs::remove_dir_all(root).unwrap();
}
