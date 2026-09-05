//! Graph queries over a loaded vault, shared by the CLI, the HTTP API, and the
//! MCP server so all three answer identically.
//!
//! Results carry hydrated [`DocumentSummary`] nodes rather than bare ids: a
//! consumer rendering an organization page should get the people who work there
//! with their titles and types in one call, not a list of ids to fetch one by
//! one.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    document::display_name, id::CanonicalId, title::title_from_id, vault::LoadedVault, Error,
    Result,
};

/// Which direction to follow edges in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Out,
    In,
    #[default]
    Both,
}

impl std::str::FromStr for Direction {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "out" => Ok(Self::Out),
            "in" => Ok(Self::In),
            "both" => Ok(Self::Both),
            other => Err(format!(
                "invalid direction `{other}` (expected out, in, or both)"
            )),
        }
    }
}

/// A node as consumers see it. Enough to render a link without a second fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSummary {
    pub id: String,
    pub r#type: String,
    pub title: String,
    pub status: Option<String>,
    pub labels: Vec<String>,
    /// Whether this node is a folder's `index` document rather than a leaf.
    ///
    /// Exported because folder indexes are ambiguous: most are containers a
    /// graph consumer wants to skip (a `people` folder is not a person), but
    /// some are genuine entities that own edges — in the snuffbox vault,
    /// `companies/snappy/customers/focusedenergy` and `projects/permaranch`
    /// both are. Kataan cannot tell the two apart, so it reports the fact and
    /// leaves the filtering to the caller instead of guessing.
    pub is_folder_index: bool,
}

/// Neighbours of one document, grouped by predicate within each direction.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Neighbors {
    pub id: String,
    /// Edges this document declares. Empty when `direction` is `in`.
    pub out: BTreeMap<String, Vec<DocumentSummary>>,
    /// Edges pointing at this document, keyed by the ontology's inverse
    /// predicate. Empty when `direction` is `out`.
    pub r#in: BTreeMap<String, Vec<DocumentSummary>>,
}

/// One authored edge, flattened for transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    pub source: String,
    pub predicate: String,
    pub target: String,
}

/// A node and link set, in the shape graph consumers already build by hand.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Subgraph {
    pub nodes: Vec<DocumentSummary>,
    pub links: Vec<Link>,
}

/// Default page size for [`documents`], and the ceiling on `limit`.
pub const DEFAULT_DOCUMENT_LIMIT: usize = 100;
pub const MAX_DOCUMENT_LIMIT: usize = 1000;

/// How much of each document to return.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Include {
    /// The summary only — enough to render a link or a row.
    #[default]
    Metadata,
    /// Summary plus the document's full metadata: its declared fields, its
    /// timestamps, its edges, and any key the vault added that kataan does not
    /// model. Free: `LoadedVault` already holds all of it in memory, so this
    /// reads nothing from disk.
    Full,
    /// Summary plus the Markdown body. This one *does* cost a filesystem read
    /// per document, because bodies are deliberately not held in memory.
    Markdown,
}

/// Restrict results to documents with an edge to `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LinkedTo {
    pub id: String,
    /// Restrict to one predicate; omit to match any edge.
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub direction: Direction,
}

/// Filters for [`documents`]. Every field is optional; an empty query lists the
/// vault, bounded by `limit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DocumentQuery {
    /// Fetch these ids specifically. Order is preserved and unresolved ids are
    /// reported in `missing` rather than failing the call.
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// Documents carrying every one of these labels.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Documents whose id is this folder or below it.
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub linked_to: Option<LinkedTo>,
    /// Keep documents whose `occurred_at` is on or after this bound.
    ///
    /// Inclusive, and compared *at the precision of the bound*: `after` and
    /// `before` of `2026-08-29` both include everything that happened that day,
    /// instants included. A bound with a clock compares against the full
    /// timestamp. Without this rule a bare day would exclude the very instants
    /// it names, since `2026-08-29` sorts before `2026-08-29T09:00:00Z`.
    ///
    /// A document with no `occurred_at` cannot satisfy a bound and is excluded
    /// whenever either is given.
    #[serde(default)]
    pub after: Option<String>,
    /// Keep documents whose `occurred_at` is on or before this bound. See
    /// [`after`](Self::after) for how precision is handled.
    #[serde(default)]
    pub before: Option<String>,
    /// Sort order. Defaults to canonical id, which is what the vault's own
    /// ordering is, so paging is stable without asking for anything.
    #[serde(default)]
    pub order: Order,
    /// Reverse the order. `order = updated_at` with this set is "what changed
    /// most recently".
    #[serde(default)]
    pub desc: bool,
    #[serde(default)]
    pub include: Include,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}

/// What [`documents`] sorts on.
///
/// Every variant falls back to canonical id for ties, so a page boundary never
/// depends on iteration order and paging cannot repeat or skip a document.
/// Documents missing the chosen timestamp sort last, ascending or descending
/// alike — absent is not "earliest", and burying them under a `desc` query
/// would be worse than keeping them together at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    #[default]
    Id,
    /// When the thing described happened. Author-set.
    OccurredAt,
    /// When the record was first written.
    CreatedAt,
    /// When the record last changed.
    UpdatedAt,
}

/// A document with optionally its body, as returned by [`documents`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentEntry {
    #[serde(flatten)]
    pub summary: DocumentSummary,
    /// Present when the query asked for `include: full`. Carries the fields a
    /// summary omits — `occurred_at`, `edges`, and everything the vault
    /// declared under `[nodes.*]`, which lands in `extra`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<crate::document::DocumentMetadata>,
    /// Present only when the query asked for `include: markdown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentPage {
    pub documents: Vec<DocumentEntry>,
    /// Requested ids that are not documents in this vault.
    pub missing: Vec<String>,
    /// Documents matching the filters before `offset`/`limit` were applied.
    pub total: usize,
}

/// List or batch-fetch documents.
///
/// One call replaces the fetch-by-id-in-a-loop pattern that made rebuilding a
/// graph artifact cost one round trip per document.
///
/// Omitting `limit` and matching more than the default is an error rather than
/// a silent truncation: a consumer rebuilding a graph must not be able to
/// mistake a partial answer for a complete one. Passing an explicit `limit` is
/// how a caller opts into paging — it then gets at most `limit` documents, with
/// `total` reporting the full match count so it knows how far to page.
pub fn documents(vault: &LoadedVault, query: &DocumentQuery) -> Result<DocumentPage> {
    // A caller who passes `limit` has chosen a page size and gets one; a caller
    // who passes none is protected from a silently truncated answer instead.
    let chose_limit = query.limit.is_some();
    let limit = query.limit.unwrap_or(DEFAULT_DOCUMENT_LIMIT);
    if limit > MAX_DOCUMENT_LIMIT {
        return Err(Error::InvalidRequest(format!(
            "limit {limit} exceeds the maximum of {MAX_DOCUMENT_LIMIT}"
        )));
    }

    // Bounds are parsed before any work: a malformed one is the caller's
    // mistake, and reporting it beats silently matching nothing.
    let after = parse_bound("after", query.after.as_deref())?;
    let before = parse_bound("before", query.before.as_deref())?;
    if let (Some(after), Some(before)) = (&after, &before) {
        // Comparable because both are RFC 3339 and one may be a prefix of the
        // other; an inverted range is empty by construction, so say so rather
        // than returning zero results the caller has to explain.
        if after > before {
            return Err(Error::InvalidRequest(format!(
                "`after` ({after}) is later than `before` ({before}), which cannot match anything"
            )));
        }
    }

    // `linked_to` is resolved once, not per candidate.
    let linked: Option<BTreeSet<CanonicalId>> = match &query.linked_to {
        Some(link) => {
            let id = CanonicalId::parse(&link.id).map_err(|error| {
                Error::InvalidRequest(format!("invalid `linked_to.id`: {error}"))
            })?;
            let neighbors = neighbors(vault, &id, link.predicate.as_deref(), link.direction)?;
            Some(
                neighbors
                    .out
                    .values()
                    .chain(neighbors.r#in.values())
                    .flatten()
                    .filter_map(|node| CanonicalId::parse(&node.id).ok())
                    .collect(),
            )
        }
        None => None,
    };

    let mut missing = Vec::new();
    let candidates: Vec<&CanonicalId> = if query.ids.is_empty() {
        vault.documents.keys().collect()
    } else {
        // Preserve request order, and report ids that do not exist rather than
        // failing the whole batch.
        let mut resolved = Vec::with_capacity(query.ids.len());
        for raw in &query.ids {
            match CanonicalId::parse(raw)
                .ok()
                .and_then(|id| vault.documents.get_key_value(&id))
            {
                Some((id, _)) => resolved.push(id),
                None => missing.push(raw.clone()),
            }
        }
        resolved
    };

    // Built once rather than per candidate: the closure below runs over every
    // document in the vault when no `ids` were given.
    let path_prefix = query
        .path_prefix
        .as_ref()
        .map(|prefix| (prefix.as_str(), format!("{prefix}/")));

    let matched: Vec<&CanonicalId> = candidates
        .into_iter()
        .filter(|id| {
            let Some(record) = vault.documents.get(*id) else {
                return false;
            };
            // Subtypes answer a query for their supertype, so `--type company`
            // returns customers too. Without `extends` in play this is an
            // equality test.
            query
                .r#type
                .as_ref()
                .is_none_or(|ty| vault.type_registry.is_a(&record.metadata.r#type, ty))
                && query
                    .status
                    .as_ref()
                    .is_none_or(|status| record.metadata.status.as_ref() == Some(status))
                && query
                    .labels
                    .iter()
                    .all(|label| record.metadata.labels.contains(label))
                && path_prefix.as_ref().is_none_or(|(prefix, with_slash)| {
                    id.as_str() == *prefix || id.as_str().starts_with(with_slash.as_str())
                })
                && linked.as_ref().is_none_or(|allowed| allowed.contains(*id))
                && within_bounds(record.metadata.occurred_at.as_deref(), &after, &before)
        })
        .collect();

    let mut matched = matched;
    sort_documents(vault, &mut matched, query.order, query.desc);

    let total = matched.len();
    let remaining = total.saturating_sub(query.offset);
    if !chose_limit && remaining > limit {
        return Err(Error::InvalidRequest(format!(
            "{remaining} documents match (offset {}), which exceeds the default limit of {limit}; \
             pass an explicit `limit` to page, or narrow the query",
            query.offset
        )));
    }

    let mut documents = Vec::new();
    for id in matched.into_iter().skip(query.offset).take(limit) {
        let Some(summary) = summarize(vault, id) else {
            continue;
        };
        // A body that cannot be read is reported, not silently dropped: the
        // caller would otherwise get fewer documents than `total` implies with
        // no signal, which is the failure the limit guard exists to prevent.
        let metadata = match query.include {
            Include::Full => vault
                .documents
                .get(id)
                .map(|record| record.metadata.clone()),
            _ => None,
        };
        let markdown = match query.include {
            Include::Metadata | Include::Full => None,
            Include::Markdown => match vault.read_markdown(id) {
                Ok(markdown) => Some(markdown),
                Err(_) => {
                    missing.push(id.as_str().to_owned());
                    continue;
                }
            },
        };
        documents.push(DocumentEntry {
            summary,
            metadata,
            markdown,
        });
    }

    Ok(DocumentPage {
        documents,
        missing,
        total,
    })
}

/// Parse a time bound, naming which one failed.
fn parse_bound(field: &str, value: Option<&str>) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(raw) => crate::time::Timestamp::parse(raw)
            .map(|timestamp| Some(timestamp.as_str().to_owned()))
            .map_err(|error| Error::InvalidRequest(format!("invalid `{field}`: {error}"))),
    }
}

/// Whether `occurred_at` falls within the bounds, comparing at each bound's own
/// precision.
///
/// RFC 3339 leaves a `full-date` as a prefix of any `date-time` on that day, so
/// a plain lexicographic test would put `2026-08-29T09:00:00Z` *after* a
/// `before` bound of `2026-08-29` and drop the instants the bound names.
/// Truncating the value to the bound's length makes both bounds mean the whole
/// day when written as a day, and the exact moment when written with a clock.
fn within_bounds(
    occurred_at: Option<&str>,
    after: &Option<String>,
    before: &Option<String>,
) -> bool {
    if after.is_none() && before.is_none() {
        return true;
    }
    // A document with no valid time cannot be shown to fall in a range.
    let Some(value) = occurred_at else {
        return false;
    };
    let at_precision = |bound: &String| {
        let end = bound.len().min(value.len());
        &value[..end]
    };
    after
        .as_ref()
        .is_none_or(|bound| at_precision(bound) >= bound.as_str())
        && before
            .as_ref()
            .is_none_or(|bound| at_precision(bound) <= bound.as_str())
}

/// Sort in place, breaking ties on canonical id so paging is stable.
fn sort_documents(vault: &LoadedVault, ids: &mut [&CanonicalId], order: Order, desc: bool) {
    if order == Order::Id {
        // Already id-ordered: `vault.documents` is a BTreeMap, and an explicit
        // `ids` list is returned in request order, which reversing still
        // honours.
        if desc {
            ids.reverse();
        }
        return;
    }

    let key = |id: &CanonicalId| -> Option<&str> {
        let metadata = &vault.documents.get(id)?.metadata;
        match order {
            Order::OccurredAt => metadata.occurred_at.as_deref(),
            Order::CreatedAt => metadata.created_at.as_deref(),
            Order::UpdatedAt => metadata.updated_at.as_deref(),
            Order::Id => None,
        }
    };

    ids.sort_by(|left, right| {
        let (left_key, right_key) = (key(left), key(right));
        // Missing sorts last in both directions, so `desc` does not bury the
        // documents that simply never carried the field.
        let ordering = match (left_key, right_key) {
            (Some(left_key), Some(right_key)) => {
                let compared = left_key.cmp(right_key);
                if desc {
                    compared.reverse()
                } else {
                    compared
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        ordering.then_with(|| left.cmp(right))
    });
}

/// Build the display summary for a document id, or `None` if it is not in the
/// vault (an edge may point at a document that was deleted).
pub fn summarize(vault: &LoadedVault, id: &CanonicalId) -> Option<DocumentSummary> {
    vault
        .documents
        .get(id)
        .map(|record| summarize_record(id, record))
}

/// Build a summary from a record the caller already holds, so iteration over
/// `documents` does not look each one up a second time.
fn summarize_record(id: &CanonicalId, record: &crate::vault::DocumentRecord) -> DocumentSummary {
    DocumentSummary {
        id: id.as_str().to_owned(),
        r#type: record.metadata.r#type.clone(),
        title: display_name(&record.metadata).unwrap_or_else(|| title_from_id(id.as_str())),
        status: record.metadata.status.clone(),
        labels: record.metadata.labels.clone(),
        is_folder_index: record.is_folder_index,
    }
}

/// Neighbours of `id`, optionally restricted to a single predicate.
///
/// `predicate` filters by the key as it appears in that direction: outgoing
/// uses the authored predicate, incoming uses the ontology's inverse.
pub fn neighbors(
    vault: &LoadedVault,
    id: &CanonicalId,
    predicate: Option<&str>,
    direction: Direction,
) -> Result<Neighbors> {
    if !vault.documents.contains_key(id) {
        return Err(Error::InvalidVaultStructure(format!(
            "unknown document `{id}`"
        )));
    }

    let collect = |grouped: BTreeMap<String, BTreeSet<CanonicalId>>| {
        grouped
            .into_iter()
            .filter(|(name, _)| predicate.is_none_or(|wanted| wanted == name))
            .map(|(name, ids)| {
                let summaries = ids.iter().filter_map(|id| summarize(vault, id)).collect();
                (name, summaries)
            })
            .collect::<BTreeMap<_, Vec<_>>>()
    };

    let out = match direction {
        Direction::Out | Direction::Both => collect(vault.graph.outgoing_all(id)),
        Direction::In => BTreeMap::new(),
    };
    let incoming = match direction {
        Direction::In | Direction::Both => collect(vault.graph.incoming_all(id)),
        Direction::Out => BTreeMap::new(),
    };

    Ok(Neighbors {
        id: id.as_str().to_owned(),
        out,
        r#in: incoming,
    })
}

/// The whole graph, optionally narrowed to some document types and predicates.
///
/// Empty `types` or `predicates` means no filter on that axis. A link is kept
/// only when both endpoints survive the type filter, so the result is always
/// internally consistent — no link ever points at a node that is not present.
pub fn subgraph(vault: &LoadedVault, types: &[String], predicates: &[String]) -> Subgraph {
    let type_matches = |ty: &str| {
        types.is_empty()
            || types
                .iter()
                .any(|allowed| vault.type_registry.is_a(ty, allowed))
    };

    let nodes: Vec<DocumentSummary> = vault
        .documents
        .iter()
        .filter(|(_, record)| type_matches(&record.metadata.r#type))
        .map(|(id, record)| summarize_record(id, record))
        .collect();
    let present: BTreeSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();

    let links = vault
        .graph
        .edges()
        .filter(|edge| {
            predicates.is_empty() || predicates.iter().any(|name| name == &edge.predicate)
        })
        .filter(|edge| {
            present.contains(edge.source.as_str()) && present.contains(edge.target.as_str())
        })
        .map(|edge| Link {
            source: edge.source.as_str().to_owned(),
            predicate: edge.predicate.clone(),
            target: edge.target.as_str().to_owned(),
        })
        .collect();

    Subgraph { nodes, links }
}

#[cfg(test)]
mod tests;
