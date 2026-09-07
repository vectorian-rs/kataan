//! `neighbors` and `subgraph`: traversal, and what an export contains once.

use super::*;

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
    let graph = subgraph(&vault, &[], &[], None).unwrap();
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

    let graph = subgraph(&vault, &[], &[], None).unwrap();

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

    let topics_only = subgraph(&vault, &["topic".to_owned()], &[], None).unwrap();
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

    let by_predicate = subgraph(&vault, &[], &["subtopic_of".to_owned()], None).unwrap();
    assert!(by_predicate
        .links
        .iter()
        .all(|link| link.predicate == "subtopic_of"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_oversized_subgraph_is_refused_rather_than_truncated() {
    let root = vault_with_edges("subgraph-limit");
    let vault = LoadedVault::load(&root).unwrap();

    let full = subgraph(&vault, &[], &[], None).unwrap();
    let total = full.nodes.len();
    assert!(total > 1, "fixture too small to exceed a limit");

    // One below the match count refuses. It must not come back with `total - 1`
    // nodes: a caller cannot tell a truncated graph from a complete one, and
    // every link into a dropped node would dangle.
    let refused = subgraph(&vault, &[], &[], Some(total - 1)).unwrap_err();
    let message = refused.to_string();
    assert!(
        message.contains(&total.to_string()) && message.contains("types"),
        "error should say how many were found and how to narrow it: {message}"
    );

    // Exactly the match count is allowed — the boundary is "more than", so a
    // caller who asks for precisely what exists is not refused it.
    assert_eq!(
        subgraph(&vault, &[], &[], Some(total)).unwrap().nodes.len(),
        total
    );

    // A filter that brings it under the limit succeeds where the unfiltered
    // call failed, which is what the error tells the caller to do.
    let topics = subgraph(&vault, &["topic".to_owned()], &[], Some(total - 1)).unwrap();
    assert!(topics.nodes.len() < total);

    // Above the hard ceiling is refused whatever the vault holds.
    assert!(subgraph(&vault, &[], &[], Some(MAX_SUBGRAPH_NODES + 1)).is_err());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn export_is_deterministic_across_rebuilds() {
    let root = vault_with_edges("subgraph-deterministic");

    let first =
        serde_json::to_value(subgraph(&LoadedVault::load(&root).unwrap(), &[], &[], None).unwrap())
            .unwrap();
    let second =
        serde_json::to_value(subgraph(&LoadedVault::load(&root).unwrap(), &[], &[], None).unwrap())
            .unwrap();

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

#[test]
fn linked_to_agrees_with_neighbors() {
    let root = vault_with_edges("documents-linked");
    let vault = LoadedVault::load(&root).unwrap();
    let systems = CanonicalId::parse("topics/systems").unwrap();

    let via_documents = documents(
        &vault,
        &q(DocumentQuery {
            linked_to: Some("topics/systems".to_owned()),
            predicate: Some("has_subtopic".to_owned()),
            direction: Direction::In,
            ..Default::default()
        }),
    )
    .unwrap();

    let via_neighbors = neighbors(&vault, &systems, Some("has_subtopic"), Direction::In).unwrap();

    let a: Vec<&str> = via_documents
        .documents
        .iter()
        .map(|d| d.summary.id.as_str())
        .collect();
    let b: Vec<&str> = via_neighbors.r#in["has_subtopic"]
        .iter()
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(a, b);
    assert_eq!(a, ["topics/rust"]);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_symmetric_peer_is_listed_once_not_in_both_directions() {
    let root = vault_with_edges("symmetric-once");
    let vault = LoadedVault::load(&root).unwrap();

    // `notes/field-notes` authored `related_to -> topics/rust`. Seen from the
    // non-authoring side the peer used to appear under BOTH `out` and `in`,
    // so a consumer rendered it twice.
    for id in ["topics/rust", "notes/field-notes"] {
        let id = CanonicalId::parse(id).unwrap();
        let result = neighbors(&vault, &id, Some("related_to"), Direction::Both).unwrap();
        let occurrences: usize = result
            .out
            .values()
            .chain(result.r#in.values())
            .map(|peers| peers.len())
            .sum();
        assert_eq!(occurrences, 1, "`{id}` listed its symmetric peer twice");
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_reciprocally_declared_edge_exports_once() {
    let root = vault_with_edges("reciprocal-once");
    // Declare the same symmetric relationship from the other side too, which
    // an author or agent may easily do.
    let rust = CanonicalId::parse("topics/rust").unwrap();
    let note = CanonicalId::parse("notes/field-notes").unwrap();
    mutate::add_edge(&root, &rust, "related_to", &note).unwrap();

    let vault = LoadedVault::load(&root).unwrap();
    let graph = subgraph(&vault, &[], &[], None).unwrap();
    let related: Vec<_> = graph
        .links
        .iter()
        .filter(|link| link.predicate == "related_to")
        .collect();

    assert_eq!(
        related.len(),
        1,
        "one relationship exported as {} links: {related:?}",
        related.len()
    );

    std::fs::remove_dir_all(root).unwrap();
}
