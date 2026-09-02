use super::*;

/// A vault whose ontology constrains `person`, exercising each field type.
fn vault_with_node_schemas(name: &str) -> std::path::PathBuf {
    let root = crate::test_support::unique_temp_dir(name);
    crate::init::init_vault(&root, "Test").unwrap();
    let ontology = fs::read_to_string(root.join("ontology.toml")).unwrap();
    fs::write(
        root.join("ontology.toml"),
        format!(
            "{ontology}\n\
[nodes.person]\n\
required = [\"linkedin\"]\n\n\
[nodes.person.fields]\n\
linkedin = {{ type = \"string\" }}\n\
born = {{ type = \"date\" }}\n\
seen_at = {{ type = \"instant\" }}\n\
employment = {{ type = \"array\", items = \"interval\" }}\n\
mentor = {{ type = \"reference\", to = [\"person\"] }}\n"
        ),
    )
    .unwrap();
    root
}

fn write_person(root: &std::path::Path, slug: &str, extra: &str) {
    fs::write(root.join(format!("people/{slug}.md")), "# P\n").unwrap();
    fs::write(
        root.join(format!("people/{slug}.toml")),
        format!("type = \"person\"\nmarkdown = \"{slug}.md\"\n{extra}"),
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(root).unwrap();
}

#[test]
fn node_schemas_enforce_required_fields_and_types() {
    let root = vault_with_node_schemas("schema-basics");

    // Missing a required field.
    write_person(&root, "nolink", "");
    assert!(codes_reported(&root).contains(&codes::MISSING_REQUIRED_FIELD.to_owned()));

    // Wrong primitive type.
    write_person(&root, "nolink", "linkedin = 42\n");
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    // A date field holding prose — the real shape this vault already contains.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nborn = \"pending approval\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_TIMESTAMP.to_owned()));

    // `instant` refuses a value that is only day-precise.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nseen_at = \"2026-08-29\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    // A well-formed document passes.
    write_person(
        &root,
        "nolink",
        "linkedin = \"x\"\nborn = \"1979-05-18\"\nseen_at = \"2026-08-29T12:00:00Z\"\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn intervals_may_be_open_but_not_backwards() {
    let root = vault_with_node_schemas("schema-intervals");

    // An open interval is a fact about the world, not an error.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nfrom = \"2020-01-01\"\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    // Ending before it starts is.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nfrom = \"2020-01-01\"\nto = \"2019-01-01\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_INTERVAL.to_owned()));

    // So is a missing `from`.
    write_person(
        &root,
        "open",
        "linkedin = \"x\"\n\n[[employment]]\nto = \"2019-01-01\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::INVALID_INTERVAL.to_owned()));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn references_must_resolve_and_respect_their_target_types() {
    let root = vault_with_node_schemas("schema-references");
    write_person(
        &root,
        "mentee",
        "linkedin = \"x\"\nmentor = \"people/ghost\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::UNRESOLVED_FIELD_REFERENCE.to_owned()));

    // Pointing at a real document of the wrong type is also refused.
    fs::write(root.join("topics/rust.md"), "# R\n").unwrap();
    fs::write(
        root.join("topics/rust.toml"),
        "type = \"topic\"\nmarkdown = \"rust.md\"\n",
    )
    .unwrap();
    write_person(
        &root,
        "mentee",
        "linkedin = \"x\"\nmentor = \"topics/rust\"\n",
    );
    assert!(codes_reported(&root).contains(&codes::FIELD_TYPE_MISMATCH.to_owned()));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn undeclared_fields_stay_legal() {
    let root = vault_with_node_schemas("schema-open-world");

    // Schemas constrain what they declare. An unknown key must still pass, or
    // adding a schema would undo the unknown-key preservation in 86cb3a8.
    write_person(
        &root,
        "extra",
        "linkedin = \"x\"\nwebsite = \"example.com\"\ncompany_id = 7\n",
    );
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    // A type with no schema at all is entirely unconstrained.
    fs::write(root.join("notes/free.md"), "# N\n").unwrap();
    fs::write(
        root.join("notes/free.toml"),
        "type = \"note\"\nmarkdown = \"free.md\"\nanything = \"goes\"\n",
    )
    .unwrap();
    crate::rebuild::rebuild_indexes(&root).unwrap();
    assert!(
        validate(&root).unwrap().is_ok(),
        "{:?}",
        codes_reported(&root)
    );

    fs::remove_dir_all(root).unwrap();
}
