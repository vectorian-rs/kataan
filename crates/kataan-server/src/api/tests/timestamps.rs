use super::*;

const SIGNED_DATES: &[&str] = &[
    "+026-01-01",
    "-026-01-01",
    "2026-+1-01",
    "2026--1-01",
    "2026-01-+1",
    "2026-01--1",
];

#[tokio::test]
async fn signed_dates_in_bounds_and_writes_are_bad_requests() {
    let root = test_vault();
    let app = test_app(&root);
    let created = json_request(
        app.clone(), "POST", "/api/documents",
        serde_json::json!({"type": "note", "title": "Dated", "body": "valid", "occurred_at": "2024-02-29"}),
    ).await;
    assert_eq!(created.status(), StatusCode::CREATED);

    for date in SIGNED_DATES {
        for bound in ["after", "before"] {
            let encoded = date.replace('+', "%2B");
            let response = request(
                app.clone(),
                "GET",
                &format!("/api/documents?{bound}={encoded}"),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bound}={date}");
        }
        for (method, uri) in [
            ("POST", "/api/documents"),
            ("PATCH", "/api/documents/notes/dated"),
        ] {
            let response = json_request(
                app.clone(), method, uri,
                serde_json::json!({"type": "note", "title": "Invalid", "body": "invalid", "occurred_at": date}),
            ).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{method} {date}"
            );
        }
    }
    let response = request(app.clone(), "GET", "/api/documents/notes/dated").await;
    let document: serde_json::Value = json_response(response).await;
    assert_eq!(document["metadata"]["occurred_at"], "2024-02-29");
    assert_eq!(document["markdown"], "valid");
    let response = request(
        app,
        "GET",
        "/api/documents?after=2024-02-29&before=2024-02-29&order=occurred_at",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let listing: serde_json::Value = json_response(response).await;
    assert_eq!(listing["documents"].as_array().unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn stored_signed_dates_do_not_panic_when_filtered_or_sorted() {
    let root = test_vault();
    for (index, date) in SIGNED_DATES.iter().enumerate() {
        let slug = format!("signed-{index}");
        fs::write(root.join(format!("notes/{slug}.md")), "# Hand authored\n").unwrap();
        fs::write(
            root.join(format!("notes/{slug}.toml")),
            format!("type = \"note\"\nmarkdown = \"{slug}.md\"\noccurred_at = \"{date}\"\n"),
        )
        .unwrap();
    }
    let app = test_app(&root);
    let response = request(app.clone(), "GET", "/api/documents?order=occurred_at").await;
    assert_eq!(response.status(), StatusCode::OK);
    let listing: serde_json::Value = json_response(response).await;
    for index in 0..SIGNED_DATES.len() {
        assert!(listing["documents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|doc| doc["id"] == format!("notes/signed-{index}")));
    }
    let response = request(
        app,
        "GET",
        "/api/documents?after=0000-01-01&order=occurred_at",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let listing: serde_json::Value = json_response(response).await;
    assert!(
        listing["documents"].as_array().unwrap().is_empty(),
        "invalid stored dates must not satisfy a bound"
    );
    fs::remove_dir_all(root).unwrap();
}
