use std::collections::BTreeMap;

use schemars::{schema_for, JsonSchema};
use serde::Serialize;
use serde_json::Value;

use crate::{
    constants::{ACTOR_VALUES, STATUS_VALUES},
    document::DocumentMetadata,
    index::{FolderIndex, VaultConfig},
    ontology::{EdgePredicate, Ontology},
    types::TypeDefinition,
    vault::LoadedVault,
};

#[derive(Debug, Clone, Serialize)]
pub struct TomlSchemaResponse {
    pub kind: String,
    pub schema: Value,
    pub constraints: SchemaConstraints,
    pub toml_template: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SchemaConstraints {
    pub allowed_status: Vec<String>,
    pub allowed_actors: Vec<String>,
    pub allowed_types: Vec<String>,
    pub allowed_edge_predicates: Vec<String>,
    pub notes: Vec<String>,
}

pub fn schema_response(kind: &str, vault: Option<&LoadedVault>) -> Option<TomlSchemaResponse> {
    let constraints = constraints(vault);
    match kind {
        "document" => Some(response::<DocumentMetadata>(
            kind,
            constraints,
            "type = \"project\"\nmarkdown = \"example.md\"\n",
        )),
        "folder-index" => Some(response::<FolderIndex>(
            kind,
            constraints,
            "type = \"project\"\nmarkdown = \"index.md\"\nname = \"Folder name\"\ndefault_type = \"project\"\n",
        )),
        "vault" => Some(response::<VaultConfig>(
            kind,
            constraints,
            "schema_version = \"0.1.0\"\nname = \"My Vault\"\n\n[type_folders]\nproject = \"projects\"\n",
        )),
        "type-definition" => Some(response::<TypeDefinition>(
            kind,
            constraints,
            "type = \"type-definition\"\nname = \"project\"\nfolder = \"projects\"\nmarkdown = \"project.md\"\n",
        )),
        "ontology" => Some(response::<Ontology>(
            kind,
            constraints,
            "schema_version = \"0.1.0\"\n\n[edges.related_to]\nfrom = [\"*\"]\nto = [\"*\"]\nsymmetric = true\ncardinality = \"many-to-many\"\n",
        )),
        "edge-predicate" => Some(response::<EdgePredicate>(
            kind,
            constraints,
            "from = [\"project\"]\nto = [\"topic\"]\ncardinality = \"many-to-many\"\n",
        )),
        _ => None,
    }
}

fn response<T: JsonSchema>(
    kind: &str,
    constraints: SchemaConstraints,
    toml_template: &str,
) -> TomlSchemaResponse {
    TomlSchemaResponse {
        kind: kind.to_owned(),
        schema: serde_json::to_value(schema_for!(T)).expect("schema serializes"),
        constraints,
        toml_template: toml_template.to_owned(),
    }
}

fn constraints(vault: Option<&LoadedVault>) -> SchemaConstraints {
    let mut allowed_types = Vec::new();
    let mut allowed_edge_predicates = Vec::new();

    if let Some(vault) = vault {
        allowed_types = vault.index.type_folders.keys().cloned().collect::<Vec<_>>();
        allowed_edge_predicates = vault.ontology.edges.keys().cloned().collect::<Vec<_>>();
    }

    SchemaConstraints {
        allowed_status: STATUS_VALUES.iter().map(|value| (*value).to_owned()).collect(),
        allowed_actors: ACTOR_VALUES.iter().map(|value| (*value).to_owned()).collect(),
        allowed_types,
        allowed_edge_predicates,
        notes: BTreeMap::from([(
            "code".to_owned(),
            "code/ is a non-document tool folder and is exempt from Markdown/TOML sidecar, folder index, loader, and Merkle rules.".to_owned(),
        )])
        .into_values()
        .collect(),
    }
}
