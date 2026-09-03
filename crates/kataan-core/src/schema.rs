use std::collections::BTreeMap;

use schemars::{schema_for, JsonSchema};
use serde::Serialize;
use serde_json::Value;

use crate::{
    constants::{ACTOR_VALUES, STATUS_VALUES},
    document::DocumentMetadata,
    index::{FolderIndex, VaultConfig},
    ontology::{EdgePredicate, FieldType, NodeSchema, Ontology},
    types::TypeDefinition,
    vault::LoadedVault,
};

#[derive(Debug, Clone, Serialize)]
pub struct TomlSchemaResponse {
    pub kind: String,
    pub schema: Value,
    pub constraints: SchemaConstraints,
    pub toml_template: String,
    /// The vault's `[nodes.<kind>]` declaration, when `kind` names a document
    /// type that has one.
    ///
    /// `schema` above describes kataan's own metadata struct — the same shape
    /// for every document. This is what makes *this* type different, and the
    /// write boundary rejects documents that do not satisfy it. Without it a
    /// caller could only discover the rules by breaking them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_schema: Option<NodeSchema>,
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
            "schema_version = \"0.1.0\"\nname = \"My Vault\"\n\n[type_folders]\nintake = \"intake\"\nproject = \"projects\"\ntype-definition = \"type\"\ncode = \"code\"\n",
        )),
        "type-definition" => Some(response::<TypeDefinition>(
            kind,
            constraints,
            "type = \"type-definition\"\nname = \"article\"\nfolder = \"articles\"\nicon = \"Newspaper\"\nmarkdown = \"article.md\"\n",
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
        // Otherwise `kind` may name a document type the vault declares. Asking
        // for `person` should answer "what does a person need", not 404.
        other => document_type_response(other, constraints, vault?),
    }
}

/// The schema for one of the vault's own document types.
fn document_type_response(
    type_name: &str,
    constraints: SchemaConstraints,
    vault: &LoadedVault,
) -> Option<TomlSchemaResponse> {
    if !vault.type_registry.contains(type_name) && !vault.ontology.nodes.contains_key(type_name) {
        return None;
    }
    let node_schema = vault.ontology.nodes.get(type_name).cloned();
    let mut base = response::<DocumentMetadata>(
        type_name,
        constraints,
        &type_toml_template(type_name, node_schema.as_ref()),
    );
    base.node_schema = node_schema;
    Some(base)
}

/// A minimum sidecar for `type_name`, including every field its node schema
/// requires, so the template is something a caller can actually write rather
/// than one that would be rejected.
fn type_toml_template(type_name: &str, node_schema: Option<&NodeSchema>) -> String {
    let mut template = format!("type = \"{type_name}\"\nmarkdown = \"example.md\"\n");
    let Some(schema) = node_schema else {
        return template;
    };
    for field in &schema.required {
        let declared = schema.fields.get(field).map(|field| field.r#type);
        template.push_str(&format!("{field} = {}\n", example_for(declared)));
    }
    template
}

/// A placeholder of the right TOML shape for a declared field type.
fn example_for(field_type: Option<FieldType>) -> &'static str {
    match field_type {
        Some(FieldType::Integer) => "0",
        Some(FieldType::Number) => "0.0",
        Some(FieldType::Boolean) => "false",
        Some(FieldType::Date) => "\"2026-08-29\"",
        Some(FieldType::Instant) => "\"2026-08-29T12:00:00Z\"",
        Some(FieldType::Interval) => "{ from = \"2026-08-29\" }",
        Some(FieldType::Reference) => "\"folder/document-id\"",
        Some(FieldType::Array) => "[]",
        Some(FieldType::Table) => "{}",
        Some(FieldType::String) | None => "\"\"",
    }
}

/// The vault's whole model in one response: every document type with the fields
/// it declares, every edge predicate, and the type-level graph they form.
///
/// Deliberately one call rather than a walk. The model is small — it is the
/// ontology and the type registry, not the documents — so an agent can hold all
/// of it before writing anything, instead of discovering the rules by being
/// rejected. `links` mirrors the shape of `query::subgraph`, so the same code
/// that reads the data graph reads the type graph.
#[derive(Debug, Clone, Serialize)]
pub struct OntologyResponse {
    pub types: Vec<OntologyType>,
    pub edges: Vec<OntologyEdge>,
    pub links: Vec<OntologyLink>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OntologyType {
    pub name: String,
    /// The supertype, if any. A subtype satisfies any rule written for its
    /// supertype, including an edge's `from`/`to` and a `--type` filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Every location this type may occupy, as vault-root-relative patterns.
    pub folders: Vec<String>,
    /// Fields the write boundary requires.
    pub required: Vec<String>,
    /// Declared fields, by name.
    pub fields: BTreeMap<String, crate::ontology::FieldSchema>,
    /// How many documents of this type the vault currently holds, folder
    /// indexes included.
    pub document_count: usize,
    /// How many of those are folder indexes rather than leaf entities.
    ///
    /// Reported rather than subtracted, because kataan cannot tell which of
    /// them are containers and which are real entities that own edges — the
    /// same reason `is_folder_index` is surfaced on every document summary.
    /// Subtract it if you want leaves; do not assume it is noise.
    pub folder_index_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OntologyEdge {
    pub predicate: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverse: Option<String>,
    pub symmetric: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One legal `source --predicate--> target` at the *type* level: what may be
/// connected to what, as opposed to what currently is.
#[derive(Debug, Clone, Serialize)]
pub struct OntologyLink {
    pub source: String,
    pub predicate: String,
    pub target: String,
}

pub fn ontology_response(vault: &LoadedVault) -> OntologyResponse {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut folder_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for record in vault.documents.values() {
        *counts.entry(record.metadata.r#type.as_str()).or_default() += 1;
        if record.is_folder_index {
            *folder_counts
                .entry(record.metadata.r#type.as_str())
                .or_default() += 1;
        }
    }

    // Every type the vault knows about, whether it comes from `type/` or from
    // an `[nodes.*]` block with no definition file.
    let mut names: Vec<&str> = vault
        .type_registry
        .definitions
        .keys()
        .map(String::as_str)
        .chain(vault.ontology.nodes.keys().map(String::as_str))
        .collect();
    names.sort_unstable();
    names.dedup();

    let types = names
        .iter()
        .map(|name| {
            let definition = vault.type_registry.definitions.get(*name);
            let node = vault.ontology.nodes.get(*name);
            OntologyType {
                name: (*name).to_owned(),
                extends: definition.and_then(|definition| definition.extends.clone()),
                folders: definition
                    .map(|definition| definition.folders.clone())
                    .unwrap_or_default(),
                required: node.map(|node| node.required.clone()).unwrap_or_default(),
                fields: node.map(|node| node.fields.clone()).unwrap_or_default(),
                document_count: counts.get(*name).copied().unwrap_or(0),
                folder_index_count: folder_counts.get(*name).copied().unwrap_or(0),
            }
        })
        .collect();

    let mut edges = Vec::new();
    let mut links = Vec::new();
    for (predicate, edge) in &vault.ontology.edges {
        for source in &edge.from {
            for target in &edge.to {
                links.push(OntologyLink {
                    source: source.clone(),
                    predicate: predicate.clone(),
                    target: target.clone(),
                });
            }
        }
        edges.push(OntologyEdge {
            predicate: predicate.clone(),
            from: edge.from.clone(),
            to: edge.to.clone(),
            inverse: edge.inverse.clone(),
            symmetric: edge.symmetric,
            cardinality: edge.cardinality.clone(),
            description: edge.description.clone(),
        });
    }

    OntologyResponse {
        types,
        edges,
        links,
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
        node_schema: None,
    }
}

fn constraints(vault: Option<&LoadedVault>) -> SchemaConstraints {
    let mut allowed_types = Vec::new();
    let mut allowed_edge_predicates = Vec::new();

    if let Some(vault) = vault {
        allowed_types = vault
            .index
            .type_folders
            .keys()
            .filter(|ty| ty.as_str() != crate::constants::TYPE_CODE)
            .cloned()
            .collect::<Vec<_>>();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault with one constrained type, built the way `mutate`'s tests do.
    fn vault_with_person_schema(name: &str) -> std::path::PathBuf {
        let root = crate::test_support::unique_temp_dir(name);
        crate::init::init_vault(&root, "Test").unwrap();
        let path = root.join("ontology.toml");
        let existing = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{existing}

[nodes.person]
required = ["email", "started_on"]

[nodes.person.fields]
email = {{ type = "string", description = "Primary contact address." }}
started_on = {{ type = "date" }}
seniority = {{ type = "integer" }}
"#
            ),
        )
        .unwrap();
        root
    }

    /// The write boundary rejects a document that violates `[nodes.*]`, so a
    /// caller has to be able to read those rules first. Before this, `schema`
    /// returned only kataan's own metadata struct — the same for every type —
    /// and the vault's actual constraints were undiscoverable.
    #[test]
    fn schema_for_a_vault_type_returns_its_declared_fields() {
        let root = vault_with_person_schema("schema-person");
        let vault = crate::vault::LoadedVault::load(&root).unwrap();

        let response = schema_response("person", Some(&vault)).expect("person is a vault type");
        let node = response.node_schema.expect("person declares a node schema");
        assert_eq!(node.required, vec!["email", "started_on"]);
        assert_eq!(
            node.fields["email"].description.as_deref(),
            Some("Primary contact address.")
        );

        // The template is something the caller can actually write: every
        // required field is present, with a placeholder of the right TOML type.
        let template = &response.toml_template;
        assert!(template.contains("type = \"person\""), "{template}");
        assert!(template.contains("email = \"\""), "{template}");
        assert!(
            template.contains("started_on = \"2026-08-29\""),
            "{template}"
        );

        // A type with no `[nodes.*]` block still answers, rather than 404ing.
        let note = schema_response("note", Some(&vault)).expect("note is a vault type");
        assert!(note.node_schema.is_none());
        // Something that is neither a kataan kind nor a vault type does not.
        assert!(schema_response("nonsense", Some(&vault)).is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

    /// The whole model in one call, so an agent can hold it before writing.
    #[test]
    fn ontology_response_describes_types_and_the_type_level_graph() {
        let root = vault_with_person_schema("schema-ontology");
        let vault = crate::vault::LoadedVault::load(&root).unwrap();
        let response = ontology_response(&vault);

        let person = response
            .types
            .iter()
            .find(|ty| ty.name == "person")
            .expect("person is listed");
        assert_eq!(person.required, vec!["email", "started_on"]);
        assert_eq!(person.folders, vec!["people"]);
        // `init_vault` writes `people/index.toml`, itself typed `person`, so the
        // count is 1 and all of it is a folder index. Reporting both is the
        // point: a caller counting entities must be able to see the difference.
        assert_eq!(person.document_count, 1);
        assert_eq!(person.folder_index_count, 1);

        // Predicates carry their permitted endpoints, and `links` is the
        // from x to product — what may connect to what, not what does.
        let related = response
            .edges
            .iter()
            .find(|edge| edge.predicate == "related_to")
            .expect("the default ontology defines related_to");
        assert!(related.symmetric);
        assert_eq!(related.from, vec!["*"]);
        assert!(response
            .links
            .iter()
            .any(|link| link.predicate == "related_to" && link.source == "*"));

        // Every declared predicate contributes at least one link.
        for edge in &response.edges {
            assert!(
                response
                    .links
                    .iter()
                    .any(|link| link.predicate == edge.predicate),
                "predicate `{}` produced no link",
                edge.predicate
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    /// The pairing that matters: read the schema, satisfy it, and the write
    /// that would otherwise be rejected succeeds.
    #[test]
    fn a_caller_that_reads_the_schema_can_write_what_the_boundary_accepts() {
        let root = vault_with_person_schema("schema-round-trip");
        let vault = crate::vault::LoadedVault::load(&root).unwrap();
        let node = schema_response("person", Some(&vault))
            .unwrap()
            .node_schema
            .unwrap();

        let blind = crate::mutate::create_document(
            &root,
            crate::mutate::NewDocument {
                r#type: "person".to_owned(),
                title: "Blind".to_owned(),
                body: "x".to_owned(),
                ..Default::default()
            },
        );
        assert!(blind.is_err(), "writing without the schema is rejected");

        // Now supply exactly what the schema said was required.
        let extra = node
            .required
            .iter()
            .map(|field| {
                let value = match node.fields.get(field).map(|field| field.r#type) {
                    Some(FieldType::Date) => toml::Value::String("2026-08-29".to_owned()),
                    _ => toml::Value::String("informed@example.com".to_owned()),
                };
                (field.clone(), value)
            })
            .collect();

        let id = crate::mutate::create_document(
            &root,
            crate::mutate::NewDocument {
                r#type: "person".to_owned(),
                title: "Informed".to_owned(),
                body: "x".to_owned(),
                extra,
                ..Default::default()
            },
        )
        .expect("a document built from the schema is accepted");
        assert_eq!(id.as_str(), "people/informed");
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }
}
