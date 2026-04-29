use std::{collections::BTreeMap, path::Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{diagnostic::Diagnostic, Error, Result};

pub const ONTOLOGY_FILE: &str = "ontology.toml";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Ontology {
    pub schema_version: String,
    #[serde(default)]
    pub edges: BTreeMap<String, EdgePredicate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EdgePredicate {
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub to: Vec<String>,
    pub inverse: Option<String>,
    #[serde(default)]
    pub symmetric: bool,
    pub cardinality: Option<String>,
    pub description: Option<String>,
}

impl Ontology {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let path = root.as_ref().join(ONTOLOGY_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| Error::TomlParse { path, source })
    }

    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for (name, predicate) in &self.edges {
            if !is_predicate_name(name) {
                diagnostics.push(
                    Diagnostic::error(
                        "invalid-ontology-entry",
                        format!("predicate `{name}` must use lowercase snake_case"),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if predicate.from.is_empty()
                || predicate.to.is_empty()
                || predicate.cardinality.is_none()
            {
                diagnostics.push(
                    Diagnostic::error(
                        "invalid-ontology-entry",
                        format!("predicate `{name}` must define from, to, and cardinality"),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if predicate.symmetric && predicate.inverse.is_some() {
                diagnostics.push(
                    Diagnostic::error(
                        "invalid-ontology-entry",
                        format!("predicate `{name}` cannot be both symmetric and inverse-backed"),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if predicate.symmetric && predicate.from != predicate.to {
                diagnostics.push(
                    Diagnostic::error(
                        "invalid-ontology-entry",
                        format!(
                            "symmetric predicate `{name}` must have matching from and to endpoints"
                        ),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if let Some(cardinality) = &predicate.cardinality {
                if !matches!(
                    cardinality.as_str(),
                    "one-to-one" | "one-to-many" | "many-to-one" | "many-to-many"
                ) {
                    diagnostics.push(
                        Diagnostic::error(
                            "invalid-ontology-entry",
                            format!("predicate `{name}` has invalid cardinality `{cardinality}`"),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
            }
        }

        diagnostics
    }
}

pub fn type_allowed(allowed: &[String], actual: &str) -> bool {
    allowed.iter().any(|ty| ty == "*" || ty == actual)
}

pub fn is_predicate_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !value.starts_with('_')
        && !value.ends_with('_')
        && !value.contains("__")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_predicate_shape() {
        let ontology = Ontology {
            schema_version: "0.1.0".to_owned(),
            edges: BTreeMap::from([(
                "bad-name".to_owned(),
                EdgePredicate {
                    from: vec!["person".to_owned()],
                    to: vec!["person".to_owned()],
                    inverse: Some("inverse".to_owned()),
                    symmetric: true,
                    cardinality: Some("bogus".to_owned()),
                    description: None,
                },
            )]),
        };

        let diagnostics = ontology.validate();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "invalid-ontology-entry"));
        assert!(diagnostics.len() >= 3);
    }
}
