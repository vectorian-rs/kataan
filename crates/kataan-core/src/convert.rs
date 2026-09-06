//! JSON to TOML, for surfaces that take JSON and write TOML.
//!
//! Both the MCP tools and the HTTP write path accept custom sidecar fields as
//! JSON and must store them as TOML. Keeping the conversion here means the two
//! surfaces cannot come to disagree about what a JSON number or a nested object
//! becomes on disk.

/// Convert a JSON value to its TOML equivalent.
///
/// `None` for JSON `null`, which TOML cannot represent at all. Callers decide
/// what that absence means: a create drops the key, a patch removes it.
/// Nulls nested inside arrays and objects are dropped, since a TOML array or
/// table has no way to hold one.
pub fn json_to_toml(value: &serde_json::Value) -> Option<toml::Value> {
    use serde_json::Value;
    Some(match value {
        Value::Null => return None,
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => toml::Value::Integer(integer),
            None => toml::Value::Float(number.as_f64()?),
        },
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Array(items) => toml::Value::Array(items.iter().filter_map(json_to_toml).collect()),
        Value::Object(fields) => toml::Value::Table(
            fields
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), json_to_toml(value)?)))
                .collect(),
        ),
    })
}
