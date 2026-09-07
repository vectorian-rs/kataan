use super::*;

use crate::query::DocumentQuery;

#[test]
fn a_list_arrives_the_same_from_a_url_and_from_json() {
    // The point of the type: two spellings, one value, so the query struct can
    // be deserialized directly on both surfaces.
    let from_url: Csv =
        serde_urlencoded::from_str::<std::collections::BTreeMap<String, Csv>>("labels=alpha,beta")
            .unwrap()
            .remove("labels")
            .unwrap();
    let from_json: Csv = serde_json::from_str(r#"["alpha", "beta"]"#).unwrap();

    assert_eq!(from_url, from_json);
    assert_eq!(from_url.as_slice(), ["alpha", "beta"]);
}

#[test]
fn blank_entries_do_not_become_empty_filters() {
    // `?labels=` is "no filter", not "documents labelled with the empty
    // string" — which would match nothing and read like a broken vault.
    let empty: Csv = serde_json::from_str(r#""""#).unwrap();
    assert!(empty.is_empty());

    let ragged: Csv = serde_json::from_str(r#""alpha, ,beta,""#).unwrap();
    assert_eq!(ragged.as_slice(), ["alpha", "beta"]);
}

#[test]
fn a_list_serializes_as_an_array_whichever_way_it_arrived() {
    // Responses never make the reader parse anything.
    let parsed: Csv = serde_json::from_str(r#""alpha,beta""#).unwrap();
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#"["alpha","beta"]"#
    );
}

#[test]
fn one_query_type_deserializes_from_both_surfaces() {
    // The whole issue in one assertion: the same filters, spelled as a URL
    // query string and as MCP tool arguments, produce the same `DocumentQuery`
    // with no per-surface adapter in between.
    let from_url: DocumentQuery = serde_urlencoded::from_str(
        "ids=notes/a,notes/b&labels=x,y&type=note&linked_to=topics/rust\
         &predicate=related_to&direction=in&include=full&limit=10&offset=5&desc=true",
    )
    .unwrap();

    let from_json: DocumentQuery = serde_json::from_value(serde_json::json!({
        "ids": ["notes/a", "notes/b"],
        "labels": ["x", "y"],
        "type": "note",
        "linked_to": "topics/rust",
        "predicate": "related_to",
        "direction": "in",
        "include": "full",
        "limit": 10,
        "offset": 5,
        "desc": true,
    }))
    .unwrap();

    assert_eq!(
        serde_json::to_value(&from_url).unwrap(),
        serde_json::to_value(&from_json).unwrap()
    );
    assert_eq!(from_url.ids.as_slice(), ["notes/a", "notes/b"]);
    assert_eq!(
        from_url.link_filter().map(|link| link.id),
        Some("topics/rust".to_owned())
    );
}

#[test]
fn an_empty_query_is_the_default_on_both_surfaces() {
    let from_url: DocumentQuery = serde_urlencoded::from_str("").unwrap();
    let from_json: DocumentQuery = serde_json::from_value(serde_json::json!({})).unwrap();
    let default = DocumentQuery::default();

    for query in [&from_url, &from_json] {
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            serde_json::to_value(&default).unwrap()
        );
        assert!(query.link_filter().is_none());
    }
}

#[test]
fn the_generated_schema_documents_the_real_limits() {
    // The doc comment on `limit` is what an agent reads — it is the MCP tool's
    // description, generated from this type rather than written out beside it.
    // So it states the actual numbers, and this keeps that prose honest: change
    // a constant without the sentence and the test says so.
    let schema = serde_json::to_value(schemars::schema_for!(DocumentQuery)).unwrap();
    let described = schema["properties"]["limit"]["description"]
        .as_str()
        .expect("limit is documented")
        .to_owned();

    for value in [
        crate::query::DEFAULT_DOCUMENT_LIMIT,
        crate::query::MAX_DOCUMENT_LIMIT,
    ] {
        assert!(
            described.contains(&value.to_string()),
            "`limit` says `{described}`, which does not mention {value}"
        );
    }
}

#[test]
fn every_filter_an_agent_can_send_is_documented() {
    // The schema is the tool description now, so a field added without a doc
    // comment ships as an undocumented argument.
    let schema = serde_json::to_value(schemars::schema_for!(DocumentQuery)).unwrap();
    let undocumented: Vec<&str> = schema["properties"]
        .as_object()
        .expect("properties")
        .iter()
        .filter(|(_, value)| value.get("description").is_none())
        .map(|(key, _)| key.as_str())
        .collect();

    assert!(
        undocumented.is_empty(),
        "undocumented query fields: {undocumented:?}"
    );
}
