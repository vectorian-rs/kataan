//! Validation tests, grouped by what they exercise.
//!
//! Fixtures shared by more than one submodule live here; the rest sit with
//! the module that uses them. Submodules reach these via `use super::*`.

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

mod documents;
mod node_schemas;
mod structure;
mod type_scopes;
mod types_and_edges;

/// Diagnostic codes a vault reports, for assertions that name one.
fn codes_reported(root: &std::path::Path) -> Vec<String> {
    validate(root)
        .unwrap()
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect()
}
