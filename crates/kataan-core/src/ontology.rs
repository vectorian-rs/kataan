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

            if let Some(inverse) = &predicate.inverse {
                // An inverse is a *label* for the reverse direction, not a
                // separately defined predicate — `owned_by.inverse = "owns"`
                // needs no `[edges.owns]`, which is how the default ontology
                // works. So existence cannot be required; ambiguity can.
                if !is_predicate_name(inverse) {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!("predicate `{name}` has an invalid inverse name `{inverse}`"),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
                // "The reverse of p is p" is symmetry, which has its own
                // spelling. The two are not interchangeable: `symmetric` makes
                // the graph reachable from both sides as outgoing, while a
                // self-inverse also records an incoming copy, so a peer shows
                // up twice.
                if inverse == name {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!(
                                "predicate `{name}` is its own inverse; use `symmetric = true`"
                            ),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                } else if self
                    .edges
                    .get(inverse)
                    .is_some_and(|other| other.inverse.as_deref() != Some(name.as_str()))
                {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::INVALID_ONTOLOGY_ENTRY,
                            format!(
                                "predicate `{name}` claims inverse `{inverse}`, which is a \
                                 predicate with a different inverse"
                            ),
                        )
                        .with_path(ONTOLOGY_FILE),
                    );
                }
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

        // A label reached from two predicates makes `incoming_all` return the
        // union of two distinct relations under one key, with no way to tell
        // them apart — the ambiguity the per-predicate check above describes,
        // but only visible across the whole edge set.
        let mut inverse_owners: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, predicate) in &self.edges {
            let Some(inverse) = predicate.inverse.as_deref() else {
                continue;
            };
            if let Some(previous) = inverse_owners.insert(inverse, name) {
                diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_ONTOLOGY_ENTRY,
                        format!(
                            "predicates `{previous}` and `{name}` both use inverse `{inverse}`; \
                             their incoming edges would be indistinguishable"
                        ),
                    )
                    .with_path(ONTOLOGY_FILE),
                );
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

/// Whether `actual` satisfies a permitted-type list.
///
/// Matches `*`, an exact name, or any supertype reachable through `extends`, so
/// a rule written `from = ["company"]` accepts a `customer` without every
/// subtype having to be listed in `ontology.toml`.
pub fn type_allowed(
    allowed: &[String],
    actual: &str,
    registry: &crate::types::TypeRegistry,
) -> bool {
    allowed
        .iter()
        .any(|ty| ty == "*" || ty == actual || registry.is_a(actual, ty))
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
        let require = |ok: bool, expected: &str| -> std::result::Result<Vec<String>, Diagnostic> {
            if ok {
                Ok(Vec::new())
            } else {
                Err(mismatch(expected))
            }
        };

        Ok(match self.r#type {
            FieldType::String => require(value.is_str(), "a string")?,
            FieldType::Integer => require(value.is_integer(), "an integer")?,
            FieldType::Number => require(value.is_float() || value.is_integer(), "a number")?,
            FieldType::Boolean => require(value.is_bool(), "a boolean")?,
            FieldType::Table => require(value.is_table(), "a table")?,
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
    registry: &crate::types::TypeRegistry,
) -> Vec<Diagnostic> {
    let Some(schema) = ontology.nodes.get(&metadata.r#type) else {
        return Vec::new();
    };
    // Serializing gives kataan's own keys without hand-maintaining a name map.
    // It is not used for `extra`, because a bare TOML datetime round-trips
    // through `Serialize` as a table rather than staying a datetime.
    let own = match toml::Table::try_from(metadata) {
        Ok(own) => own,
        Err(error) => {
            return vec![Diagnostic::warning(
                codes::INVALID_ONTOLOGY_ENTRY,
                format!("could not read fields of `{}`: {error}", metadata.r#type),
            )]
        }
    };
    let field_value = |name: &String| metadata.extra.get(name).or_else(|| own.get(name));
    let mut diagnostics = Vec::new();

    for field in &schema.required {
        if field_value(field).is_none() {
            diagnostics.push(Diagnostic::error(
                codes::MISSING_REQUIRED_FIELD,
                format!("type `{}` requires field `{field}`", metadata.r#type),
            ));
        }
    }

    for (field, field_schema) in &schema.fields {
        // Absent is fine unless `required` said otherwise, handled above.
        let Some(value) = field_value(field) else {
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
                    if !field_schema.to.is_empty()
                        && !type_allowed(&field_schema.to, target_type, registry)
                    {
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

    /// An edge predicate with the given inverse, otherwise unconstrained.
    fn predicate(inverse: Option<&str>) -> EdgePredicate {
        EdgePredicate {
            from: vec!["*".to_owned()],
            to: vec!["*".to_owned()],
            inverse: inverse.map(str::to_owned),
            symmetric: false,
            cardinality: Some("many-to-many".to_owned()),
            description: None,
        }
    }

    fn ontology_with(edges: &[(&str, Option<&str>)]) -> Ontology {
        Ontology {
            schema_version: "0.1.0".to_owned(),
            nodes: BTreeMap::new(),
            edges: edges
                .iter()
                .map(|(name, inverse)| ((*name).to_owned(), predicate(*inverse)))
                .collect(),
        }
    }

    fn codes(ontology: &Ontology) -> Vec<String> {
        ontology.validate().into_iter().map(|d| d.code).collect()
    }

    #[test]
    fn an_inverse_label_needs_no_predicate_of_its_own() {
        // How the shipped default ontology works: `derived`, `mentioned_in`,
        // `has_subtopic` are reverse-direction labels, not defined predicates.
        assert!(codes(&ontology_with(&[("owned_by", Some("owns"))])).is_empty());
        // A reciprocal pair is equally fine.
        assert!(codes(&ontology_with(&[
            ("owned_by", Some("owns")),
            ("owns", Some("owned_by")),
        ]))
        .is_empty());
    }

    #[test]
    fn ambiguous_inverses_are_reported() {
        // The label names a real predicate pointing somewhere else.
        assert!(!codes(&ontology_with(&[
            ("owned_by", Some("owns")),
            ("owns", Some("something_else")),
        ]))
        .is_empty());

        // Two predicates sharing one label: `incoming_all` would return the
        // union of two distinct relations under one key.
        assert!(!codes(&ontology_with(&[
            ("authored", Some("authored_by")),
            ("wrote", Some("authored_by")),
        ]))
        .is_empty());

        // "The reverse of p is p" is symmetry, and has its own spelling — the
        // two behave differently in the graph.
        assert!(!codes(&ontology_with(&[("related_to", Some("related_to"))])).is_empty());
    }

    #[test]
    fn the_shipped_default_ontology_validates() {
        let ontology: Ontology =
            toml::from_str(include_str!("../templates/default-ontology.toml")).unwrap();
        assert!(
            ontology.validate().is_empty(),
            "default ontology reports: {:?}",
            codes(&ontology)
        );
    }

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
