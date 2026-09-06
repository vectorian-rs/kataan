//! Query tests, split into the two questions `query` answers.
//!
//! The fixture is shared: both halves need a vault with edges in it.

mod graph;
mod listing;

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

// --- documents() -----------------------------------------------------------

fn q(query: DocumentQuery) -> DocumentQuery {
    query
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
