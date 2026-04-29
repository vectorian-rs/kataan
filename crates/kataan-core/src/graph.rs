use std::collections::{BTreeMap, BTreeSet};

use crate::{id::CanonicalId, vault::LoadedDocument, Result};

#[derive(Debug, Clone, Default)]
pub struct VaultGraph {
    pub documents: BTreeMap<CanonicalId, LoadedDocument>,
    belongs_to: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
    children_of: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
    related_to: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
    sources: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
    derived_from: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
}

impl VaultGraph {
    pub fn build(documents: impl IntoIterator<Item = LoadedDocument>) -> Result<Self> {
        let mut graph = Self::default();

        for document in documents {
            graph.documents.insert(document.id.clone(), document);
        }

        let edges = graph
            .documents
            .iter()
            .map(|(id, document)| {
                (
                    id.clone(),
                    document.metadata.belongs_to.clone(),
                    document.metadata.related_to.clone(),
                    document.metadata.sources.clone(),
                )
            })
            .collect::<Vec<_>>();

        for (id, belongs_to, related_to, sources) in edges {
            for target in belongs_to {
                let target =
                    CanonicalId::parse(target).map_err(|_| crate::Error::ValidationFailed)?;
                graph
                    .belongs_to
                    .entry(id.clone())
                    .or_default()
                    .insert(target.clone());
                graph
                    .children_of
                    .entry(target)
                    .or_default()
                    .insert(id.clone());
            }

            for target in related_to {
                let target =
                    CanonicalId::parse(target).map_err(|_| crate::Error::ValidationFailed)?;
                graph
                    .related_to
                    .entry(id.clone())
                    .or_default()
                    .insert(target.clone());
                graph
                    .related_to
                    .entry(target)
                    .or_default()
                    .insert(id.clone());
            }

            for target in sources {
                let target =
                    CanonicalId::parse(target).map_err(|_| crate::Error::ValidationFailed)?;
                graph
                    .sources
                    .entry(id.clone())
                    .or_default()
                    .insert(target.clone());
                graph
                    .derived_from
                    .entry(target)
                    .or_default()
                    .insert(id.clone());
            }
        }

        Ok(graph)
    }

    pub fn parents_of(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.belongs_to.get(id).cloned().unwrap_or_default()
    }

    pub fn children_of(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.children_of.get(id).cloned().unwrap_or_default()
    }

    pub fn related_to(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.related_to.get(id).cloned().unwrap_or_default()
    }

    pub fn sources_of(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.sources.get(id).cloned().unwrap_or_default()
    }

    pub fn derived_from(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.derived_from.get(id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use crate::document::DocumentMetadata;

    use super::*;

    #[test]
    fn computes_children_by_reversing_belongs_to() {
        let project = document("projects/kataan-redesign", "project");
        let mut note = document("notes/project-brief", "note");
        note.metadata.belongs_to = vec!["projects/kataan-redesign".to_owned()];

        let graph = VaultGraph::build([project, note]).unwrap();

        let project_id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        let note_id = CanonicalId::parse("notes/project-brief").unwrap();

        assert_eq!(
            graph.children_of(&project_id),
            BTreeSet::from([note_id.clone()])
        );
        assert_eq!(graph.parents_of(&note_id), BTreeSet::from([project_id]));
    }

    #[test]
    fn treats_related_to_as_undirected() {
        let mut note = document("notes/rust-on-arm64", "note");
        note.metadata.related_to = vec!["topics/rust".to_owned()];
        let topic = document("topics/rust", "topic");

        let graph = VaultGraph::build([note, topic]).unwrap();

        let note_id = CanonicalId::parse("notes/rust-on-arm64").unwrap();
        let topic_id = CanonicalId::parse("topics/rust").unwrap();

        assert_eq!(
            graph.related_to(&note_id),
            BTreeSet::from([topic_id.clone()])
        );
        assert_eq!(graph.related_to(&topic_id), BTreeSet::from([note_id]));
    }

    #[test]
    fn computes_reverse_source_provenance() {
        let raw = document("raw/pasted-chat", "raw");
        let mut note = document("notes/summary", "note");
        note.metadata.sources = vec!["raw/pasted-chat".to_owned()];

        let graph = VaultGraph::build([raw, note]).unwrap();

        let raw_id = CanonicalId::parse("raw/pasted-chat").unwrap();
        let note_id = CanonicalId::parse("notes/summary").unwrap();

        assert_eq!(graph.sources_of(&note_id), BTreeSet::from([raw_id.clone()]));
        assert_eq!(graph.derived_from(&raw_id), BTreeSet::from([note_id]));
    }

    fn document(id: &str, ty: &str) -> LoadedDocument {
        let id = CanonicalId::parse(id).unwrap();
        LoadedDocument {
            metadata: DocumentMetadata {
                r#type: ty.to_owned(),
                status: None,
                markdown: format!("{}.md", id.slug()),
                markdown_checksum: None,
                aliases: Vec::new(),
                labels: Vec::new(),
                edges: Default::default(),
                belongs_to: Vec::new(),
                related_to: Vec::new(),
                sources: Vec::new(),
                created_by: None,
                last_updated_by: None,
            },
            markdown: String::new(),
            ancestors: id.ancestors().into_iter().map(str::to_owned).collect(),
            is_folder_index: false,
            id,
        }
    }
}
