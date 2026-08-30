use std::{collections::BTreeMap, path::Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{diagnostic::Diagnostic, diagnostic_codes as codes, Error, Result};

pub const ONTOLOGY_FILE: &str = "ontology.toml";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Ontology {
    pub schema_version: String,
    #[serde(default)]
    pub edges: BTreeMap<String, EdgePredicate>,
    /// Per-type field schemas. Absent types are unconstrained, which is what
    /// keeps this additive: a vault adopts schemas one type at a time.
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeSchema>,
}

/// What a document of one type must and may carry.
///
/// Schemas constrain what is *declared*; they never forbid what is not. An
/// undeclared field stays legal, so adding a schema cannot retroactively
/// invalidate documents or undo the unknown-key preservation in 86cb3a8.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct NodeSchema {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FieldSchema {
    pub r#type: FieldType,
    /// Element type, for `array`.
    pub items: Option<FieldType>,
    /// Allowed target types, for `reference`. Empty means any type.
    #[serde(default)]
    pub to: Vec<String>,
    pub description: Option<String>,
}

/// The type vocabulary kataan ships. The vault composes these; kataan knows
/// what an interval is, not that employment is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
    /// Any precision the time vocabulary allows: `2006` through
    /// `2026-08-29T12:00:00Z`.
    Date,
    /// A fixed point on the timeline — day precision is not enough.
    Instant,
    /// A table with `from` and an optional `to`; an absent `to` is an open
    /// interval, which is a fact about the world, not an error.
    Interval,
    /// The canonical id of another document in this vault.
    Reference,
    Array,
    Table,
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
                        codes::INVALID_ONTOLOGY_ENTRY,
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
                        codes::INVALID_ONTOLOGY_ENTRY,
                        format!("predicate `{name}` must define from, to, and cardinality"),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if predicate.symmetric && predicate.inverse.is_some() {
                diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_ONTOLOGY_ENTRY,
                        format!("predicate `{name}` cannot be both symmetric and inverse-backed"),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
            }

            if predicate.symmetric && predicate.from != predicate.to {
                diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_ONTOLOGY_ENTRY,
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
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!("predicate `{name}` has invalid cardinality `{cardinality}`"),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
            }
        }

        for (type_name, schema) in &self.nodes {
            for (field, definition) in &schema.fields {
                if definition.r#type == FieldType::Array && definition.items.is_none() {
                    diagnostics.push(
                        Diagnostic::warning(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!(
                                "`nodes.{type_name}.fields.{field}` is an array without `items`; elements are unchecked"
                            ),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
                if !definition.to.is_empty()
                    && definition.r#type != FieldType::Reference
                    && definition.items != Some(FieldType::Reference)
                {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!(
                                "`nodes.{type_name}.fields.{field}` sets `to` but is not a reference"
                            ),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
            }
            // A required field that is never described is almost always a typo.
            for field in &schema.required {
                if !schema.fields.contains_key(field) {
                    diagnostics.push(
                        Diagnostic::warning(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!(
                                "`nodes.{type_name}` requires `{field}` but does not define it under `fields`"
                            ),
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

/// Where a schema violation was found, and what was wrong.
impl FieldSchema {
    /// Check one value against this field's declared type. An empty `Vec` means
    /// the value is fine; entries are reference targets whose existence the
    /// caller must confirm, since that needs the whole document set.
    pub fn check(
        &self,
        field: &str,
        value: &toml::Value,
    ) -> std::result::Result<Vec<String>, Diagnostic> {
        let mismatch = |expected: &str| {
            Diagnostic::error(
                codes::FIELD_TYPE_MISMATCH,
                format!("`{field}` must be {expected}, found {}", value.type_str()),
            )
        };
        let require = |ok: bool, expected: &str| -> std::result::Result<(), Diagnostic> {
            if ok {
                Ok(())
            } else {
                Err(mismatch(expected))
            }
        };

        Ok(match self.r#type {
            FieldType::String => {
                require(value.is_str(), "a string")?;
                Vec::new()
            }
            FieldType::Integer => {
                require(value.is_integer(), "an integer")?;
                Vec::new()
            }
            FieldType::Number => {
                require(value.is_float() || value.is_integer(), "a number")?;
                Vec::new()
            }
            FieldType::Boolean => {
                require(value.is_bool(), "a boolean")?;
                Vec::new()
            }
            FieldType::Table => {
                require(value.is_table(), "a table")?;
                Vec::new()
            }
            FieldType::Date | FieldType::Instant => {
                let raw = value
                    .as_str()
                    .ok_or_else(|| mismatch("a timestamp string"))?;
                let parsed = crate::time::Timestamp::parse(raw).map_err(|error| {
                    Diagnostic::error(codes::INVALID_TIMESTAMP, format!("`{field}`: {error}"))
                })?;
                if self.r#type == FieldType::Instant
                    && parsed.precision() != crate::time::Precision::Instant
                {
                    return Err(Diagnostic::error(
                        codes::FIELD_TYPE_MISMATCH,
                        format!(
                            "`{field}` must be an exact instant, but `{raw}` is only {:?} precision",
                            parsed.precision()
                        ),
                    ));
                }
                Vec::new()
            }
            FieldType::Interval => {
                check_interval(field, value)?;
                Vec::new()
            }
            FieldType::Reference => {
                vec![value
                    .as_str()
                    .ok_or_else(|| mismatch("a document id"))?
                    .to_owned()]
            }
            FieldType::Array => {
                let items = value.as_array().ok_or_else(|| mismatch("an array"))?;
                let Some(item_type) = self.items else {
                    return Ok(Vec::new());
                };
                let element = FieldSchema {
                    r#type: item_type,
                    items: None,
                    to: self.to.clone(),
                    description: None,
                };
                let mut references = Vec::new();
                for item in items {
                    references.extend(element.check(field, item)?);
                }
                references
            }
        })
    }
}

/// An interval is `{ from, to? }`. An absent `to` is deliberately legal: an
/// open interval means "still true", which is a fact about the world rather
/// than missing data. Policy about open intervals belongs to the vault.
fn check_interval(field: &str, value: &toml::Value) -> std::result::Result<(), Diagnostic> {
    let invalid = |message: String| Diagnostic::error(codes::INVALID_INTERVAL, message);
    let table = value.as_table().ok_or_else(|| {
        invalid(format!(
            "`{field}` must be a table with `from` and optional `to`"
        ))
    })?;
    let from = table
        .get("from")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid(format!("`{field}` is missing `from`")))?;
    let from = crate::time::Timestamp::parse(from)
        .map_err(|error| invalid(format!("`{field}.from`: {error}")))?;

    let Some(to) = table.get("to") else {
        return Ok(());
    };
    let to = to
        .as_str()
        .ok_or_else(|| invalid(format!("`{field}.to` must be a timestamp string")))?;
    let to = crate::time::Timestamp::parse(to)
        .map_err(|error| invalid(format!("`{field}.to`: {error}")))?;

    // ISO-8601 sorts lexicographically, so this holds across mixed precision.
    if to.as_str() < from.as_str() {
        return Err(invalid(format!(
            "`{field}` ends ({to}) before it starts ({from})"
        )));
    }
    Ok(())
}

/// Validate one document's fields against its type's schema.
///
/// Runs after the vault walk so every document is visible, which is what makes
/// `reference` checkable — and means one implementation covers both the
/// top-level and nested document walkers.
///
/// Diagnostics come back without a path; the caller attaches it.
pub fn validate_node_fields(
    ontology: &Ontology,
    metadata: &crate::document::DocumentMetadata,
    known_document_types: &BTreeMap<String, String>,
) -> Vec<Diagnostic> {
    let Some(schema) = ontology.nodes.get(&metadata.r#type) else {
        return Vec::new();
    };
    // Serialize once rather than hand-mapping field names: `DocumentMetadata`
    // already flattens `extra`, so this is the document exactly as authored,
    // and schemas can address kataan's own keys and the vault's alike.
    let Ok(fields) = toml::Table::try_from(metadata) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();

    for field in &schema.required {
        if !fields.contains_key(field) {
            diagnostics.push(Diagnostic::error(
                codes::MISSING_REQUIRED_FIELD,
                format!("type `{}` requires field `{field}`", metadata.r#type),
            ));
        }
    }

    for (field, field_schema) in &schema.fields {
        // Absent is fine unless `required` said otherwise, handled above.
        let Some(value) = fields.get(field) else {
            continue;
        };
        match field_schema.check(field, value) {
            Ok(references) => {
                for target in references {
                    let Some(target_type) = known_document_types.get(&target) else {
                        diagnostics.push(Diagnostic::error(
                            codes::UNRESOLVED_FIELD_REFERENCE,
                            format!("`{field}` references `{target}`, which does not exist"),
                        ));
                        continue;
                    };
                    if !field_schema.to.is_empty() && !type_allowed(&field_schema.to, target_type) {
                        diagnostics.push(Diagnostic::error(
                            codes::FIELD_TYPE_MISMATCH,
                            format!(
                                "`{field}` references `{target}` of type `{target_type}`, which is not among {:?}",
                                field_schema.to
                            ),
                        ));
                    }
                }
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_predicate_shape() {
        let ontology = Ontology {
            schema_version: "0.1.0".to_owned(),
            nodes: BTreeMap::new(),
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
            .any(|diagnostic| diagnostic.code == codes::INVALID_ONTOLOGY_ENTRY));
        assert!(diagnostics.len() >= 3);
    }
}
