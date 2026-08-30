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

// --- documents() -----------------------------------------------------------

fn q(query: DocumentQuery) -> DocumentQuery {
    query
}

#[test]
fn batch_fetch_preserves_order_and_reports_misses() {
    let root = vault_with_edges("documents-batch");
    let vault = LoadedVault::load(&root).unwrap();

    let page = documents(
        &vault,
        &q(DocumentQuery {
            ids: vec![
                "topics/systems".to_owned(),
                "notes/does-not-exist".to_owned(),
                "topics/rust".to_owned(),
            ],
            ..Default::default()
        }),
    )
    .unwrap();

    // Request order, not vault order — `systems` sorts after `rust`.
    let ids: Vec<&str> = page
        .documents
        .iter()
        .map(|d| d.summary.id.as_str())
        .collect();
    assert_eq!(ids, ["topics/systems", "topics/rust"]);
    // A missing id is reported, not fatal to the batch.
    assert_eq!(page.missing, ["notes/does-not-exist"]);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn filters_narrow_the_listing() {
    let root = vault_with_edges("documents-filters");
    let vault = LoadedVault::load(&root).unwrap();

    let topics = documents(
        &vault,
        &q(DocumentQuery {
            r#type: Some("topic".to_owned()),
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(topics.documents.iter().all(|d| d.summary.r#type == "topic"));
    assert!(topics.documents.len() >= 2);

    // No text query needed, unlike search.
    let under_topics = documents(
        &vault,
        &q(DocumentQuery {
            path_prefix: Some("topics".to_owned()),
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(under_topics
        .documents
        .iter()
        .all(|d| d.summary.id.starts_with("topics")));

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
            linked_to: Some(LinkedTo {
                id: "topics/systems".to_owned(),
                predicate: Some("has_subtopic".to_owned()),
                direction: Direction::In,
            }),
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
fn bodies_are_opt_in() {
    let root = vault_with_edges("documents-include");
    let vault = LoadedVault::load(&root).unwrap();

    let bare = documents(&vault, &q(DocumentQuery::default())).unwrap();
    assert!(bare.documents.iter().all(|d| d.markdown.is_none()));

    let full = documents(
        &vault,
        &q(DocumentQuery {
            include: Include::Markdown,
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(full.documents.iter().all(|d| d.markdown.is_some()));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unbounded_query_refuses_rather_than_truncating() {
    let root = vault_with_edges("documents-limit");
    let vault = LoadedVault::load(&root).unwrap();

    let total = documents(&vault, &q(DocumentQuery::default()))
        .unwrap()
        .total;
    assert!(total > 2, "fixture too small to page");

    // No `limit` given: the caller has not thought about page size, so a
    // partial answer must not be handed back as if it were complete.
    let unbounded = documents(
        &vault,
        &q(DocumentQuery {
            limit: None,
            ..Default::default()
        }),
    );
    if total > DEFAULT_DOCUMENT_LIMIT {
        assert!(unbounded.is_err(), "unbounded over-default query truncated");
    }

    // A limit above the hard ceiling is rejected outright.
    assert!(documents(
        &vault,
        &q(DocumentQuery {
            limit: Some(MAX_DOCUMENT_LIMIT + 1),
            ..Default::default()
        })
    )
    .is_err());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_explicit_limit_pages_from_the_first_document() {
    let root = vault_with_edges("documents-paging");
    let vault = LoadedVault::load(&root).unwrap();

    let all = documents(
        &vault,
        &q(DocumentQuery {
            limit: Some(MAX_DOCUMENT_LIMIT),
            ..Default::default()
        }),
    )
    .unwrap();
    let total = all.total;
    assert!(total > 2, "fixture too small to page");

    // Walk the whole vault one document at a time. Page 1 must be reachable —
    // it was not: the guard compared `total - offset` against `limit`, so every
    // offset but the last errored and only the tail was retrievable.
    let mut seen = Vec::new();
    for offset in 0..total {
        let page = documents(
            &vault,
            &q(DocumentQuery {
                limit: Some(1),
                offset,
                ..Default::default()
            }),
        )
        .unwrap_or_else(|error| panic!("offset {offset} failed: {error}"));
        assert_eq!(page.documents.len(), 1, "offset {offset}");
        assert_eq!(page.total, total, "total must not depend on paging");
        seen.push(page.documents[0].summary.id.clone());
    }

    let expected: Vec<String> = all.documents.iter().map(|d| d.summary.id.clone()).collect();
    assert_eq!(seen, expected, "paging must cover every document, in order");

    // Reading past the end is empty, not an error.
    let past = documents(
        &vault,
        &q(DocumentQuery {
            limit: Some(1),
            offset: total + 10,
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(past.documents.is_empty());
    assert_eq!(past.total, total);

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
    let graph = subgraph(&vault, &[], &[]);
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
