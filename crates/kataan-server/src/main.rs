use axum::{routing::get, Json, Router};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/api/health", get(health));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .expect("bind server");
    println!("kataan-server listening on http://127.0.0.1:3001");
    axum::serve(listener, app).await.expect("serve");
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}
