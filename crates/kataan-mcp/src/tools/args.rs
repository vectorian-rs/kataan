//! Pulling typed arguments out of an MCP call's untyped JSON.
//!
//! An agent sends whatever it sends, so every one of these has to say what it
//! does with a missing or wrong-typed value rather than assume one.

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use kataan_core::id::CanonicalId;

pub(super) fn to_pretty<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("failed to serialize response")
}

pub(super) fn str_arg(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key).ok_or_else(|| anyhow!("missing string argument `{key}`"))
}

pub(super) fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(super) fn str_vec(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// `Some(list)` only when `key` is present, so an omitted field leaves a patch
/// field unchanged rather than clearing it.
pub(super) fn opt_str_vec(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).map(|_| str_vec(args, key))
}

/// Extra sidecar fields from an object-valued argument. `mutate` rejects any
/// key kataan defines, so no filtering is needed here.
pub(super) fn extra_fields(
    args: &Value,
    key: &str,
) -> std::collections::BTreeMap<String, toml::Value> {
    match args.get(key).and_then(json_to_toml) {
        Some(toml::Value::Table(fields)) => fields.into_iter().collect(),
        _ => Default::default(),
    }
}

/// Custom fields for an update, where a JSON `null` means "remove this key"
/// rather than "no value" — the distinction `extra_fields` has no need for,
/// since a create has nothing to remove.
pub(super) fn patch_fields(
    args: &Value,
    key: &str,
) -> std::collections::BTreeMap<String, Option<toml::Value>> {
    match args.get(key) {
        Some(Value::Object(fields)) => fields
            .iter()
            .map(|(name, value)| (name.clone(), json_to_toml(value)))
            .collect(),
        _ => Default::default(),
    }
}

/// `direction` defaults to the enum's own default rather than restating it.
pub(super) fn opt_direction(args: &Value) -> Result<kataan_core::query::Direction> {
    match opt_str(args, "direction") {
        Some(value) => value.parse().map_err(|error: String| anyhow!(error)),
        None => Ok(kataan_core::query::Direction::default()),
    }
}

/// Convert a JSON tool argument to TOML. JSON null has no TOML representation,
/// so null-valued entries are dropped rather than written as something else.
use kataan_core::convert::json_to_toml;

pub(super) fn parse_id(args: &Value, key: &str) -> Result<CanonicalId> {
    CanonicalId::parse(str_arg(args, key)?).map_err(|error| anyhow!("invalid `{key}`: {error}"))
}
