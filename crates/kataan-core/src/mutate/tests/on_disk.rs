//! What a write actually leaves on disk: keys kataan does not model, the
//! timestamps it stamps, and the writes it declines to make at all.

use super::*;

#[test]
fn unknown_keys_are_readable_through_document_metadata() {
    let root = temp_vault("expose-extra");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);

    let record = Vault::open(&root)
        .unwrap()
        .load_document_record(&id)
        .unwrap();

    assert_eq!(
        record.metadata.extra["linkedin"].as_str(),
        Some("https://example.com/in/jane")
    );
    assert!(record.metadata.extra.contains_key("employment"));
    // Keys kataan models must not leak into `extra`.
    for reserved in RESERVED_KEYS {
        assert!(
            !record.metadata.extra.contains_key(*reserved),
            "`{reserved}` leaked into extra"
        );
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rebuild_indexes_is_idempotent_over_custom_keys() {
    let root = temp_vault("rebuild-extra");
    let id = create_document(&root, note("Jane", "hello")).unwrap();
    write_custom_keys(&root, &id);

    crate::rebuild::rebuild_indexes(&root).unwrap();
    let once = std::fs::read_to_string(root.join(id.toml_path())).unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();
    let twice = std::fs::read_to_string(root.join(id.toml_path())).unwrap();

    assert_eq!(once, twice);
    assert_custom_keys_intact(&root, &id);
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn writes_stamp_transaction_time_in_iso8601() {
    let root = temp_vault("time-stamps");
    let id = create_document(&root, note("Stamped", "before")).unwrap();

    let created = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    let created_at = created["created_at"].as_str().unwrap().to_owned();
    assert_eq!(created["updated_at"].as_str(), Some(created_at.as_str()));
    // ISO-8601 UTC, never the bare epoch the old helper produced.
    assert!(
        crate::time::Timestamp::parse(&created_at).is_ok(),
        "{created_at}"
    );
    assert!(created_at.ends_with('Z'), "{created_at}");

    update_document(
        &root,
        &id,
        Some("after".to_owned()),
        DocumentPatch::default(),
    )
    .unwrap();
    let updated = read_sidecar_table(&root.join(id.toml_path())).unwrap();
    // created_at is immutable; updated_at moves.
    assert_eq!(updated["created_at"].as_str(), Some(created_at.as_str()));
    assert!(crate::time::Timestamp::parse(updated["updated_at"].as_str().unwrap()).is_ok());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resending_identical_content_is_not_a_change() {
    let root = temp_vault("time-noop-body");
    let id = create_document(&root, note("Same", "identical")).unwrap();
    let path = root.join(id.toml_path());
    let before = std::fs::read_to_string(&path).unwrap();

    // A caller resending the body it already has must not move updated_at.
    update_document(
        &root,
        &id,
        Some("identical".to_owned()),
        DocumentPatch::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_legacy_epoch_in_kataan_toml_loads_then_heals_on_rebuild() {
    let root = temp_vault("time-migration");
    // Recreate the pre-1.0 shape: `updated_at` written as a bare epoch.
    let config_path = root.join(crate::constants::VAULT_CONFIG_FILE);
    let legacy = std::fs::read_to_string(&config_path).unwrap().replace(
        &format!("updated_at = \"{}\"", crate::time::iso8601_utc_now()),
        "",
    );
    let mut table: toml::Table = legacy.parse().unwrap();
    table.insert(
        "updated_at".to_owned(),
        toml::Value::String("1788013953".to_owned()),
    );
    write_sidecar_table(&config_path, &table).unwrap();

    // Lenient read: the vault still loads and validates with the old value.
    assert!(Vault::open(&root).is_ok());
    assert!(crate::validate::validate(&root).unwrap().is_ok());

    // Strict write: the next rebuild replaces it with ISO-8601.
    rebuild::rebuild_indexes(&root).unwrap();
    let healed = read_sidecar_table(&config_path).unwrap();
    let value = healed["updated_at"].as_str().unwrap();
    assert!(
        crate::time::Timestamp::parse(value).is_ok(),
        "epoch was not healed: {value}"
    );

    std::fs::remove_dir_all(root).unwrap();
}
