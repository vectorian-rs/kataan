//! Validation tests, grouped by what they exercise.
//!
//! Shared fixtures live here; each submodule reaches them via `use super::*`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::*;

fn write_root_index(root: &Path) {
    fs::write(
        root.join("ontology.toml"),
        r#"schema_version = "0.1.0"

[edges.related_to]
from = ["*"]
to = ["*"]
symmetric = true
cardinality = "many-to-many"

[edges.derived_from]
from = ["*"]
to = ["intake", "note"]
inverse = "derived"
cardinality = "many-to-many"
"#,
    )
    .unwrap();
    fs::write(
        root.join(VAULT_CONFIG_FILE),
        r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("type")).unwrap();
    fs::write(root.join("type/index.md"), "# Types\n").unwrap();
    fs::write(
        root.join("type/index.toml"),
        "type = \"type-definition\"\nname = \"Types\"\nmarkdown = \"index.md\"\n",
    )
    .unwrap();
    for (ty, folder) in [
        ("intake", "intake"),
        ("project", "projects"),
        ("person", "people"),
        ("note", "notes"),
        ("topic", "topics"),
        ("type-definition", "type"),
    ] {
        fs::write(
            root.join("type").join(format!("{ty}.md")),
            format!("# {ty}\n"),
        )
        .unwrap();
        fs::write(
                root.join("type").join(format!("{ty}.toml")),
                format!(
                    "type = \"type-definition\"\nname = \"{ty}\"\nfolder = \"{folder}\"\nmarkdown = \"{ty}.md\"\n"
                ),
            )
            .unwrap();
    }
}

fn unique_temp_dir() -> PathBuf {
    crate::test_support::unique_temp_dir("validate")
}

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

fn codes_reported(root: &std::path::Path) -> Vec<String> {
    validate(root)
        .unwrap()
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect()
}

mod documents;
mod node_schemas;
mod structure;
mod types_and_edges;
