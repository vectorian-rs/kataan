use super::*;

fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = crate::test_support::unique_temp_dir(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.toml");
    std::fs::write(&path, contents).unwrap();
    path
}

fn table(toml_text: &str) -> toml::Table {
    toml_text.parse().unwrap()
}

#[test]
fn comments_grouping_and_inline_arrays_survive_a_derived_write() {
    let original = "\
# Why this note exists.
type = \"note\"
markdown = \"commented.md\"

# Author-set, grouped deliberately.
status = \"active\"
labels = [\"alpha\", \"beta\"]
";
    let path = write_temp("edit-preserve", original);

    // Exactly what a rebuild does: stamp the checksum, touch nothing else.
    set_keys(
        &path,
        &table("markdown = \"commented.md\"\nmarkdown_checksum = \"blake3:abc\"\n"),
    )
    .unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("# Why this note exists."), "{after}");
    assert!(
        after.contains("# Author-set, grouped deliberately."),
        "{after}"
    );
    // The inline array stays inline: it was not part of the write, so it is not
    // re-rendered. This is the failure that made a rebuild reformat the vault.
    assert!(after.contains("labels = [\"alpha\", \"beta\"]"), "{after}");
    assert!(
        after.contains("\n\n# Author-set"),
        "blank line lost: {after}"
    );
    assert!(
        after.contains("markdown_checksum = \"blake3:abc\""),
        "{after}"
    );

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_unmodelled_key_survives_a_derived_write() {
    // The folder-index bug: a rewrite from a fixed struct dropped every key the
    // struct did not model, including `[edges]`, on every rebuild.
    let original = "\
type = \"note\"
name = \"Notes\"
status = \"active\"
labels = [\"curated\"]

[edges]
related_to = [\"topics/rust\"]
";
    let path = write_temp("edit-unmodelled", original);

    set_keys(&path, &table("folder_checksum = \"blake3:zzz\"\n")).unwrap();

    let after: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(after["status"].as_str(), Some("active"));
    assert_eq!(after["labels"].as_array().unwrap().len(), 1);
    assert_eq!(after["folder_checksum"].as_str(), Some("blake3:zzz"));
    assert_eq!(
        after["edges"]["related_to"][0].as_str(),
        Some("topics/rust"),
        "edges on a folder index were destroyed"
    );

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_key_added_after_a_table_does_not_land_inside_it() {
    // TOML is positional: a bare key emitted after a `[table]` header belongs
    // to that table. If `toml_edit` appended it textually, `folder_checksum`
    // would silently become `edges.folder_checksum`.
    let original = "\
type = \"note\"

[edges]
related_to = [\"topics/rust\"]
";
    let path = write_temp("edit-ordering", original);

    set_keys(&path, &table("folder_checksum = \"blake3:zzz\"\n")).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    let parsed: toml::Table = after.parse().unwrap();
    assert_eq!(
        parsed["folder_checksum"].as_str(),
        Some("blake3:zzz"),
        "key was swallowed by the preceding table: {after}"
    );
    assert!(parsed["edges"].get("folder_checksum").is_none(), "{after}");

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn arrays_of_tables_stay_arrays_of_tables() {
    // A folder index's `documents` is `[[documents]]` blocks. Rendering it as
    // an inline array would rewrite every folder index in the vault into a
    // shape no author would have written.
    let original = "type = \"note\"\nname = \"Notes\"\n";
    let path = write_temp("edit-aot", original);

    set_keys(
        &path,
        &table("[[documents]]\nslug = \"one\"\n\n[[documents]]\nslug = \"two\"\n"),
    )
    .unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("[[documents]]"), "{after}");
    let parsed: toml::Table = after.parse().unwrap();
    assert_eq!(parsed["documents"].as_array().unwrap().len(), 2);

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_comment_above_a_changed_key_outlives_the_change() {
    let original = "\
type = \"note\"

# This comment explains the status below.
status = \"active\"
";
    let path = write_temp("edit-comment-changed", original);

    set_keys(&path, &table("status = \"done\"\n")).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.contains("# This comment explains the status below."),
        "{after}"
    );
    assert!(after.contains("status = \"done\""), "{after}");

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn apply_table_removes_what_it_does_not_name_and_set_keys_does_not() {
    let original = "type = \"note\"\nstatus = \"active\"\ngoing = \"away\"\n";

    let kept = write_temp("edit-keep", original);
    set_keys(&kept, &table("status = \"done\"\n")).unwrap();
    let after: toml::Table = std::fs::read_to_string(&kept).unwrap().parse().unwrap();
    assert_eq!(after["going"].as_str(), Some("away"));
    assert_eq!(after["type"].as_str(), Some("note"));

    let pruned = write_temp("edit-prune", original);
    apply_table(&pruned, &table("type = \"note\"\nstatus = \"done\"\n")).unwrap();
    let after: toml::Table = std::fs::read_to_string(&pruned).unwrap().parse().unwrap();
    assert!(
        after.get("going").is_none(),
        "removed key survived: {after:?}"
    );
    assert_eq!(after["status"].as_str(), Some("done"));

    std::fs::remove_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(pruned.parent().unwrap()).unwrap();
}

#[test]
fn a_nested_change_leaves_its_siblings_untouched() {
    let original = "\
type = \"person\"

[rate_card]
# The currency was agreed in the contract.
currency = \"EUR\"
amount = 100
";
    let path = write_temp("edit-nested", original);

    set_keys(
        &path,
        &table("[rate_card]\ncurrency = \"EUR\"\namount = 120\n"),
    )
    .unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.contains("# The currency was agreed in the contract."),
        "sibling comment lost: {after}"
    );
    assert!(after.contains("amount = 120"), "{after}");

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_unchanged_write_does_not_touch_the_file() {
    let original = "type = \"note\"\nstatus = \"active\"\n";
    let path = write_temp("edit-noop", original);
    let before = std::fs::metadata(&path).unwrap();

    set_keys(&path, &table("status = \"active\"\n")).unwrap();

    let after = std::fs::metadata(&path).unwrap();
    // Inode, not content: a rewrite with identical bytes passes a content check
    // while still paying the two fsyncs the skip exists to avoid.
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&before),
        std::os::unix::fs::MetadataExt::ino(&after),
        "file was rewritten despite no change"
    );

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
