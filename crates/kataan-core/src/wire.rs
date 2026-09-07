//! Shapes that let one query type be deserialized straight off every surface.
//!
//! HTTP query strings and JSON tool arguments disagree about how to spell a
//! list: `?labels=a,b` versus `"labels": ["a", "b"]`. That disagreement is the
//! only reason [`crate::query::DocumentQuery`] used to be hand-assembled three
//! times, once per surface, with each copy free to drift.
//!
//! [`Csv`] accepts both, so the query type can be deserialized directly by
//! `axum::extract::Query`, by `serde_json::from_value`, and by clap.

use std::fmt;

use schemars::JsonSchema;
use serde::{
    de::{SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};

/// A list of strings written either as one comma-separated value or as an
/// array.
///
/// Serializes as an array: a response should not make the reader parse
/// anything. Only the *input* is permissive, and only because a URL has no way
/// to say "array".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct Csv(pub Vec<String>);

impl Csv {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<String> {
        self.0
    }
}

impl From<Vec<String>> for Csv {
    fn from(values: Vec<String>) -> Self {
        Self(values)
    }
}

impl<'de> Deserialize<'de> for Csv {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(CsvVisitor)
    }
}

struct CsvVisitor;

impl<'de> Visitor<'de> for CsvVisitor {
    type Value = Csv;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a comma-separated string or an array of strings")
    }

    fn visit_str<E>(self, value: &str) -> Result<Csv, E> {
        // Empty entries are dropped rather than becoming an empty-string
        // filter: `?labels=` and `?labels=a,,b` both mean what they look like.
        Ok(Csv(value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Csv, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element::<String>()? {
            values.push(value);
        }
        Ok(Csv(values))
    }

    fn visit_unit<E>(self) -> Result<Csv, E> {
        Ok(Csv::default())
    }

    fn visit_none<E>(self) -> Result<Csv, E> {
        Ok(Csv::default())
    }
}

#[cfg(test)]
mod tests;
