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
fn full_returns_declared_fields_without_reading_the_body() {
    let root = vault_with_edges("documents-full");
    // A custom key is exactly what a consumer declares under `[nodes.*]` and
    // then cannot see through a summary.
    let id = CanonicalId::parse("topics/rust").unwrap();
    let path = root.join(id.toml_path());
    let mut sidecar: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    sidecar.insert(
        "homepage".to_owned(),
        toml::Value::String("rust-lang.org".to_owned()),
    );
    sidecar.insert(
        "occurred_at".to_owned(),
        toml::Value::String("2010".to_owned()),
    );
    std::fs::write(&path, toml::to_string_pretty(&sidecar).unwrap()).unwrap();

    let vault = LoadedVault::load(&root).unwrap();
    let page = documents(
        &vault,
        &q(DocumentQuery {
            ids: vec!["topics/rust".to_owned()],
            include: Include::Full,
            ..Default::default()
        }),
    )
    .unwrap();

    let entry = &page.documents[0];
    let metadata = entry.metadata.as_ref().expect("full omitted metadata");
    // The three things a summary cannot carry.
    assert_eq!(metadata.extra["homepage"].as_str(), Some("rust-lang.org"));
    assert_eq!(metadata.occurred_at.as_deref(), Some("2010"));
    assert!(metadata.edges.contains_key("subtopic_of"), "edges missing");
    // Full is a memory read, so it must not pull the body along with it.
    assert!(entry.markdown.is_none(), "full should not read the body");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn metadata_is_omitted_unless_asked_for() {
    let root = vault_with_edges("documents-default-shape");
    let vault = LoadedVault::load(&root).unwrap();
    for include in [Include::Metadata, Include::Markdown] {
        let page = documents(
            &vault,
            &q(DocumentQuery {
                include,
                limit: Some(MAX_DOCUMENT_LIMIT),
                ..Default::default()
            }),
        )
        .unwrap();
        assert!(
            page.documents.iter().all(|d| d.metadata.is_none()),
            "{include:?} leaked full metadata"
        );
    }
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

/// Four notes with known `occurred_at` values spanning one day boundary, plus
/// one with no valid time at all.
fn vault_with_times(name: &str) -> std::path::PathBuf {
    let root = crate::test_support::unique_temp_dir(name);
    crate::init::init_vault(&root, "Test").unwrap();

    for (title, occurred_at) in [
        ("Early Day", Some("2026-08-28")),
        ("Day Itself", Some("2026-08-29")),
        ("Morning Of", Some("2026-08-29T09:00:00Z")),
        ("Evening Of", Some("2026-08-29T21:00:00Z")),
        ("Next Day", Some("2026-08-30")),
        ("Undated", None),
    ] {
        mutate::create_document(
            &root,
            NewDocument {
                r#type: "note".to_owned(),
                title: title.to_owned(),
                body: title.to_owned(),
                occurred_at: occurred_at.map(str::to_owned),
                ..Default::default()
            },
        )
        .unwrap();
    }
    root
}

fn ids(page: &DocumentPage) -> Vec<&str> {
    page.documents
        .iter()
        .map(|entry| entry.summary.id.as_str())
        .collect()
}

fn note_query(root: &std::path::Path) -> (LoadedVault, DocumentQuery) {
    (
        LoadedVault::load(root).unwrap(),
        DocumentQuery {
            r#type: Some("note".to_owned()),
            limit: Some(100),
            ..Default::default()
        },
    )
}

/// The subtle case. A `full-date` is a prefix of every `date-time` on that day,
/// so a naive lexicographic bound of `2026-08-29` would sort *before*
/// `2026-08-29T09:00:00Z` and drop the very instants it names. Bounds are
/// therefore compared at their own precision.
#[test]
fn a_day_bound_covers_the_whole_day_including_instants() {
    let root = vault_with_times("bounds-precision");
    let (vault, base) = note_query(&root);

    let single_day = documents(
        &vault,
        &DocumentQuery {
            after: Some("2026-08-29".to_owned()),
            before: Some("2026-08-29".to_owned()),
            order: Order::OccurredAt,
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(
        ids(&single_day),
        vec!["notes/day-itself", "notes/morning-of", "notes/evening-of"],
        "a bare day must cover the day and every instant within it"
    );

    // A bound carrying a clock means that exact moment, not the whole day.
    let after_noon = documents(
        &vault,
        &DocumentQuery {
            after: Some("2026-08-29T12:00:00Z".to_owned()),
            before: Some("2026-08-29T23:59:59Z".to_owned()),
            order: Order::OccurredAt,
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(ids(&after_noon), vec!["notes/evening-of"]);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounds_are_inclusive_and_exclude_undated_documents() {
    let root = vault_with_times("bounds-inclusive");
    let (vault, base) = note_query(&root);

    let from_the_28th = documents(
        &vault,
        &DocumentQuery {
            after: Some("2026-08-28".to_owned()),
            order: Order::OccurredAt,
            ..base.clone()
        },
    )
    .unwrap();
    // Inclusive: the 28th itself is in. Undated is not — a document with no
    // valid time cannot be shown to fall in a range.
    assert_eq!(from_the_28th.documents.len(), 5);
    assert_eq!(ids(&from_the_28th)[0], "notes/early-day");
    assert!(!ids(&from_the_28th).contains(&"notes/undated"));

    // With no bounds at all, the undated document is present again.
    let unbounded = documents(&vault, &base).unwrap();
    assert!(ids(&unbounded).contains(&"notes/undated"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ordering_is_stable_and_puts_missing_timestamps_last() {
    let root = vault_with_times("order");
    let (vault, base) = note_query(&root);

    let ascending = documents(
        &vault,
        &DocumentQuery {
            order: Order::OccurredAt,
            ..base.clone()
        },
    )
    .unwrap();
    // `notes` is the folder index, itself typed `note` and carrying no
    // occurred_at, so it joins the undated group at the end and sorts within it
    // by id. Folder indexes are documents; excluding them here would be
    // asserting something the query does not do.
    assert_eq!(
        ids(&ascending),
        vec![
            "notes/early-day",
            "notes/day-itself",
            "notes/morning-of",
            "notes/evening-of",
            "notes/next-day",
            "notes",
            "notes/undated",
        ]
    );

    let descending = documents(
        &vault,
        &DocumentQuery {
            order: Order::OccurredAt,
            desc: true,
            ..base.clone()
        },
    )
    .unwrap();
    // Reversed, except that the undated document stays at the end rather than
    // being promoted to the front: absent is not "earliest".
    assert_eq!(
        ids(&descending),
        vec![
            "notes/next-day",
            "notes/evening-of",
            "notes/morning-of",
            "notes/day-itself",
            "notes/early-day",
            "notes",
            "notes/undated",
        ]
    );

    std::fs::remove_dir_all(root).unwrap();
}

/// Paging has to be stable under an ordering with ties, or a page boundary
/// could repeat or skip a document. Every document here was created in the same
/// second, so `created_at` ties across all of them and only the id tiebreak
/// makes the result deterministic.
#[test]
fn paging_a_tied_ordering_visits_every_document_exactly_once() {
    let root = vault_with_times("order-paging");
    let (vault, base) = note_query(&root);

    // Seven notes: the six created above plus the `notes` folder index.
    let total = documents(&vault, &base).unwrap().total;
    assert_eq!(total, 7);

    let mut seen = Vec::new();
    for offset in (0..total).step_by(2) {
        let page = documents(
            &vault,
            &DocumentQuery {
                order: Order::CreatedAt,
                limit: Some(2),
                offset,
                ..base.clone()
            },
        )
        .unwrap();
        seen.extend(ids(&page).into_iter().map(str::to_owned));
    }

    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(seen.len(), total, "every document appears");
    assert_eq!(unique.len(), total, "and none appears twice");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_malformed_or_inverted_bound_is_a_request_error() {
    let root = vault_with_times("bounds-invalid");
    let (vault, base) = note_query(&root);

    // Reduced precision is not RFC 3339, and must be refused here as at every
    // other boundary rather than silently matching nothing.
    for bad in ["2026", "not-a-date", "2026-02-30"] {
        let result = documents(
            &vault,
            &DocumentQuery {
                after: Some(bad.to_owned()),
                ..base.clone()
            },
        );
        assert!(matches!(result, Err(Error::InvalidRequest(_))), "{bad}");
    }

    // An inverted range cannot match anything; say so rather than returning an
    // empty page the caller has to explain.
    let inverted = documents(
        &vault,
        &DocumentQuery {
            after: Some("2026-08-30".to_owned()),
            before: Some("2026-08-28".to_owned()),
            ..base.clone()
        },
    );
    assert!(matches!(inverted, Err(Error::InvalidRequest(_))));

    std::fs::remove_dir_all(root).unwrap();
}
