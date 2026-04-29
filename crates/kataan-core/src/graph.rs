use std::collections::{BTreeMap, BTreeSet};

use crate::{id::CanonicalId, ontology::Ontology, vault::LoadedDocument, Result};

#[derive(Debug, Clone, Default)]
pub struct VaultGraph {
    pub documents: BTreeMap<CanonicalId, LoadedDocument>,
    outgoing_edges: BTreeMap<CanonicalId, BTreeMap<String, BTreeSet<CanonicalId>>>,
    incoming_edges: BTreeMap<CanonicalId, BTreeMap<String, BTreeSet<CanonicalId>>>,
    path_children: BTreeMap<CanonicalId, BTreeSet<CanonicalId>>,
}

impl VaultGraph {
    pub fn build(documents: impl IntoIterator<Item = LoadedDocument>) -> Result<Self> {
        Self::build_with_ontology(documents, None)
    }

    pub fn build_with_ontology(
        documents: impl IntoIterator<Item = LoadedDocument>,
        ontology: Option<&Ontology>,
    ) -> Result<Self> {
        let mut graph = Self::default();

        for document in documents {
            graph.documents.insert(document.id.clone(), document);
        }

        graph.build_path_children();

        let edges = graph
            .documents
            .iter()
            .map(|(id, document)| (id.clone(), document.metadata.edges.clone()))
            .collect::<Vec<_>>();

        for (source, predicates) in edges {
            for (predicate_name, targets) in predicates {
                for target in targets {
                    let target =
                        CanonicalId::parse(target).map_err(|_| crate::Error::ValidationFailed)?;
                    graph
                        .outgoing_edges
                        .entry(source.clone())
                        .or_default()
                        .entry(predicate_name.clone())
                        .or_default()
                        .insert(target.clone());

                    let incoming_predicate = ontology
                        .and_then(|ontology| ontology.edges.get(&predicate_name))
                        .and_then(|predicate| {
                            if predicate.symmetric {
                                Some(predicate_name.as_str())
                            } else {
                                predicate.inverse.as_deref()
                            }
                        })
                        .unwrap_or(predicate_name.as_str())
                        .to_owned();

                    graph
                        .incoming_edges
                        .entry(target.clone())
                        .or_default()
                        .entry(incoming_predicate.clone())
                        .or_default()
                        .insert(source.clone());

                    if ontology
                        .and_then(|ontology| ontology.edges.get(&predicate_name))
                        .is_some_and(|predicate| predicate.symmetric)
                    {
                        graph
                            .outgoing_edges
                            .entry(target)
                            .or_default()
                            .entry(predicate_name.clone())
                            .or_default()
                            .insert(source.clone());
                    }
                }
            }
        }

        Ok(graph)
    }

    pub fn outgoing(&self, id: &CanonicalId, predicate: &str) -> BTreeSet<CanonicalId> {
        self.outgoing_edges
            .get(id)
            .and_then(|predicates| predicates.get(predicate))
            .cloned()
            .unwrap_or_default()
    }

    pub fn incoming(&self, id: &CanonicalId, predicate: &str) -> BTreeSet<CanonicalId> {
        self.incoming_edges
            .get(id)
            .and_then(|predicates| predicates.get(predicate))
            .cloned()
            .unwrap_or_default()
    }

    pub fn neighbors(&self, id: &CanonicalId, predicate: &str) -> BTreeSet<CanonicalId> {
        let mut neighbors = self.outgoing(id, predicate);
        neighbors.extend(self.incoming(id, predicate));
        neighbors
    }

    pub fn children_of(&self, id: &CanonicalId) -> BTreeSet<CanonicalId> {
        self.path_children.get(id).cloned().unwrap_or_default()
    }

    fn build_path_children(&mut self) {
        for id in self.documents.keys() {
            let Some((parent, _)) = id.as_str().rsplit_once('/') else {
                continue;
            };
            let Ok(parent) = CanonicalId::parse(parent) else {
                continue;
            };
            if self.documents.contains_key(&parent) {
                self.path_children
                    .entry(parent)
                    .or_default()
                    .insert(id.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        document::DocumentMetadata,
        ontology::{EdgePredicate, Ontology},
    };

    use super::*;

    #[test]
    fn computes_path_children_from_canonical_ids() {
        let folder = document("projects/company-x", "project");
        let child = document("projects/company-x/q2-launch", "project");

        let graph = VaultGraph::build([folder, child]).unwrap();

        let folder_id = CanonicalId::parse("projects/company-x").unwrap();
        let child_id = CanonicalId::parse("projects/company-x/q2-launch").unwrap();

        assert_eq!(graph.children_of(&folder_id), BTreeSet::from([child_id]));
    }

    #[test]
    fn computes_inverse_edges_from_ontology() {
        let person = document("people/jane-doe", "person");
        let mut project = document("projects/kataan-redesign", "project");
        project
            .metadata
            .edges
            .insert("owned_by".to_owned(), vec!["people/jane-doe".to_owned()]);
        let ontology = ontology();

        let graph = VaultGraph::build_with_ontology([person, project], Some(&ontology)).unwrap();

        let project_id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        let person_id = CanonicalId::parse("people/jane-doe").unwrap();

        assert_eq!(
            graph.outgoing(&project_id, "owned_by"),
            BTreeSet::from([person_id.clone()])
        );
        assert_eq!(
            graph.incoming(&person_id, "owns"),
            BTreeSet::from([project_id.clone()])
        );
        assert_eq!(
            graph.neighbors(&person_id, "owns"),
            BTreeSet::from([project_id])
        );
    }

    #[test]
    fn symmetric_edges_are_queryable_from_both_sides() {
        let mut first = document("topics/rust", "topic");
        first.metadata.edges.insert(
            "related_to".to_owned(),
            vec!["topics/local-first".to_owned()],
        );
        let second = document("topics/local-first", "topic");
        let ontology = ontology();

        let graph = VaultGraph::build_with_ontology([first, second], Some(&ontology)).unwrap();

        let rust = CanonicalId::parse("topics/rust").unwrap();
        let local_first = CanonicalId::parse("topics/local-first").unwrap();

        assert_eq!(
            graph.neighbors(&rust, "related_to"),
            BTreeSet::from([local_first.clone()])
        );
        assert_eq!(
            graph.neighbors(&local_first, "related_to"),
            BTreeSet::from([rust])
        );
    }

    fn ontology() -> Ontology {
        Ontology {
            schema_version: "0.1.0".to_owned(),
            edges: BTreeMap::from([
                (
                    "owned_by".to_owned(),
                    EdgePredicate {
                        from: vec!["project".to_owned()],
                        to: vec!["person".to_owned()],
                        inverse: Some("owns".to_owned()),
                        symmetric: false,
                        cardinality: Some("many-to-one".to_owned()),
                        description: None,
                    },
                ),
                (
                    "related_to".to_owned(),
                    EdgePredicate {
                        from: vec!["*".to_owned()],
                        to: vec!["*".to_owned()],
                        inverse: None,
                        symmetric: true,
                        cardinality: Some("many-to-many".to_owned()),
                        description: None,
                    },
                ),
            ]),
        }
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
