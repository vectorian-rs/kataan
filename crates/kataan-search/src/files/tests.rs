use super::*;
use std::path::Path;

#[test]
fn prose_data_and_source_are_indexable() {
    for path in [
        "notes/plan.txt",
        "data/rows.csv",
        "relationship-intelligence/data/companies/acme.json",
        "presentations/src/pages/deck.astro",
        "code/tool.rs",
        "code/app.ts",
        "docs/page.html",
        "config/settings.yaml",
    ] {
        assert!(is_indexable(Path::new(path)), "{path} should be indexed");
    }
}

#[test]
fn binaries_and_unknown_formats_are_not() {
    // An allow-list, so anything unrecognised is out by default — a vault
    // accumulates build output under names nobody predicted.
    for path in [
        "images/photo.jpg",
        "docs/contract.docx",
        "build/tool.o",
        "diagrams/plan.drawio",
        "no-extension-at-all",
    ] {
        assert!(!is_indexable(Path::new(path)), "{path} should be skipped");
    }
}

#[test]
fn generated_files_are_skipped_by_name() {
    // Text, with a lexer, and pure noise: matching a package name in a lockfile
    // buries the document that actually discusses that package.
    for path in [
        "apps/web/package-lock.json",
        "apps/web/bun.lock",
        "Cargo.lock",
        "pnpm-lock.yaml",
    ] {
        assert!(!is_indexable(Path::new(path)), "{path} should be skipped");
    }
    // But an ordinary file of the same extension is not.
    assert!(is_indexable(Path::new("data/records.json")));
    assert!(is_indexable(Path::new("config/app.yaml")));
}

#[test]
fn the_extension_check_ignores_case() {
    assert!(is_indexable(Path::new("notes/PLAN.TXT")));
    assert!(is_indexable(Path::new("data/ROWS.CSV")));
}
