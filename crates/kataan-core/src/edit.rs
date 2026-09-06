//! Editing TOML files in place, preserving everything the write does not mean
//! to change.
//!
//! Every write path here reads a file an author owns and puts back a derived
//! value or two. Parsing to `toml::Value` and re-rendering with
//! `to_string_pretty` loses the parts TOML carries but a value tree does not —
//! comments, blank-line grouping, whether an array was written inline — and,
//! worse, silently drops any key the projecting struct does not model. A
//! folder index rewritten from `FolderIndexToml` lost its `status`, its
//! `labels` and its entire `[edges]` table on every rebuild.
//!
//! So the derived keys are applied *onto* the parsed document rather than the
//! document being regenerated from the keys. `toml_edit` keeps the original
//! text for everything untouched.
//!
//! Two shapes, because the two callers mean different things:
//!
//! - [`set_keys`] — "these keys now have these values". Anything else in the
//!   file is the author's and stays. Used for derived fields written back into
//!   a file kataan does not own the whole of.
//! - [`apply_table`] — "the file should say exactly this". Keys absent from the
//!   table are removed. Used where the caller has computed the complete
//!   intended contents, and a removed field must actually disappear.

use std::path::Path;

use toml_edit::{DocumentMut, Item};

use crate::{write, Error, Result};

/// What happens to keys the caller did not mention.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Absent<'a> {
    /// Leave them alone — the caller is describing a change, not a file.
    Keep,
    /// Remove just these — the caller owns them but has none to write.
    Remove(&'a [&'a str]),
    /// Remove all of them — the caller is describing the whole file.
    Prune,
}

/// Set `values` on the TOML document at `path`, leaving every other key, and
/// all formatting and comments, exactly as the author wrote them.
///
/// Writes nothing when the result is byte-identical, so a no-op rebuild costs
/// no fsyncs.
pub fn set_keys(path: &Path, values: &toml::Table) -> Result<()> {
    edit_file(path, values, Absent::Keep)
}

/// Set `values`, and additionally remove any key named in `derived` that
/// `values` does not carry.
///
/// For a caller that owns a fixed set of keys but only emits the ones it
/// currently has. A folder index skips `documents` entirely once a folder is
/// empty, and a plain upsert would leave the last rebuild's `[[documents]]`
/// blocks behind for good — listing files that are gone.
pub fn set_derived(path: &Path, values: &toml::Table, derived: &[&str]) -> Result<()> {
    let stale: Vec<&str> = derived
        .iter()
        .copied()
        .filter(|key| !values.contains_key(*key))
        .collect();
    edit_file(path, values, Absent::Remove(&stale))
}

/// Make the TOML document at `path` carry exactly `table`, removing keys it
/// does not name. Formatting and comments survive for everything that stays.
pub fn apply_table(path: &Path, table: &toml::Table) -> Result<()> {
    edit_file(path, table, Absent::Prune)
}

fn edit_file(path: &Path, desired: &toml::Table, absent: Absent<'_>) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // Parsed twice on purpose. The `toml` parse yields the semantic view used
    // to decide what actually differs — and, being the same parser the rest of
    // the crate uses, it produces the `TomlParse` error a malformed vault file
    // should report. `toml_edit` then parses for the formatting.
    let current: toml::Table = text.parse().map_err(|source| Error::TomlParse {
        path: path.to_path_buf(),
        source,
    })?;
    let mut document = text.parse::<DocumentMut>().map_err(|error| {
        // Unreachable in practice: the same bytes just parsed as TOML above.
        Error::InvalidVaultStructure(format!("{}: {error}", path.display()))
    })?;

    merge(document.as_table_mut(), &current, desired, absent);
    write::atomic_write_string_if_changed(path, &document.to_string())
}

/// Apply `desired` onto `target`, using `current` to tell what already matches.
///
/// A key whose value is already what we want is skipped rather than reassigned,
/// which is what keeps its formatting: reassigning an identical value would
/// still re-render it, turning a hand-written inline array into an exploded one
/// for no change in meaning.
fn merge(
    target: &mut toml_edit::Table,
    current: &toml::Table,
    desired: &toml::Table,
    absent: Absent<'_>,
) {
    match absent {
        Absent::Keep => {}
        Absent::Remove(stale) => target.retain(|key, _| !stale.contains(&key)),
        Absent::Prune => target.retain(|key, _| desired.contains_key(key)),
    }

    for (key, want) in desired {
        match current.get(key) {
            Some(have) if have == want => continue,
            // Both sides are tables: recurse, so a change to one nested key
            // does not re-render its siblings or drop the comments between
            // them. Only for a standard table — an inline table is a single
            // value and is replaced as one.
            Some(toml::Value::Table(have)) => {
                if let (toml::Value::Table(want), Some(nested)) =
                    (want, target.get_mut(key).and_then(Item::as_table_mut))
                {
                    merge(nested, have, want, absent);
                    continue;
                }
            }
            _ => {}
        }
        set(target, key, want);
    }
}

/// Assign one key, keeping the key's own decoration — the comment written above
/// it belongs to the key, not to the value, and outlives a change of value.
fn set(target: &mut toml_edit::Table, key: &str, value: &toml::Value) {
    let item = to_item(value);
    let existing_decor = target.key(key).map(|key| key.leaf_decor().clone());
    target.insert(key, item);
    if let (Some(decor), Some(mut key)) = (existing_decor, target.key_mut(key)) {
        *key.leaf_decor_mut() = decor;
    }
}

fn to_item(value: &toml::Value) -> Item {
    match value {
        toml::Value::Table(table) => Item::Table(to_table(table)),
        // An array of tables renders as `[[key]]` blocks rather than one long
        // inline array. `documents` and `subfolders` in a folder index are
        // this shape, and inlining them would rewrite every folder index in
        // the vault into a form no author would have written.
        toml::Value::Array(items)
            if !items.is_empty() && items.iter().all(|item| item.is_table()) =>
        {
            let mut tables = toml_edit::ArrayOfTables::new();
            for item in items {
                if let toml::Value::Table(table) = item {
                    tables.push(to_table(table));
                }
            }
            Item::ArrayOfTables(tables)
        }
        other => Item::Value(to_value(other)),
    }
}

fn to_table(table: &toml::Table) -> toml_edit::Table {
    let mut out = toml_edit::Table::new();
    for (key, value) in table {
        out.insert(key, to_item(value));
    }
    out
}

fn to_value(value: &toml::Value) -> toml_edit::Value {
    match value {
        toml::Value::String(text) => text.as_str().into(),
        toml::Value::Integer(number) => (*number).into(),
        toml::Value::Float(number) => (*number).into(),
        toml::Value::Boolean(flag) => (*flag).into(),
        // kataan writes timestamps as strings, but an author may have written a
        // bare TOML datetime; keep it one rather than quoting it into a string.
        toml::Value::Datetime(stamp) => stamp
            .to_string()
            .parse::<toml_edit::Datetime>()
            .map_or_else(|_| stamp.to_string().into(), Into::into),
        toml::Value::Array(items) => items
            .iter()
            .map(to_value)
            .collect::<toml_edit::Array>()
            .into(),
        // Reached only for a table nested inside an array or an inline table,
        // where TOML has no header form available.
        toml::Value::Table(table) => {
            let mut inline = toml_edit::InlineTable::new();
            for (key, value) in table {
                inline.insert(key, to_value(value));
            }
            inline.into()
        }
    }
}

#[cfg(test)]
mod tests;
