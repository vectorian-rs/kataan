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

/// Build the display summary for a document id, or `None` if it is not in the
/// vault (an edge may point at a document that was deleted).
pub fn summarize(vault: &LoadedVault, id: &CanonicalId) -> Option<DocumentSummary> {
    let record = vault.documents.get(id)?;
    Some(DocumentSummary {
        id: id.as_str().to_owned(),
        r#type: record.metadata.r#type.clone(),
        title: display_name(&record.metadata).unwrap_or_else(|| title_from_id(id.as_str())),
        status: record.metadata.status.clone(),
        labels: record.metadata.labels.clone(),
        is_folder_index: record.is_folder_index,
    })
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
    let type_matches = |ty: &str| types.is_empty() || types.iter().any(|allowed| allowed == ty);

    let nodes: Vec<DocumentSummary> = vault
        .documents
        .iter()
        .filter(|(_, record)| type_matches(&record.metadata.r#type))
        .filter_map(|(id, _)| summarize(vault, id))
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
