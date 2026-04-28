use std::path::PathBuf;

use axum::{
    routing::{get, post},
    Router,
};
use clap::Parser;

mod api;
mod state;

use state::AppState;

#[derive(Debug, Parser)]
#[command(name = "kataan-server")]
#[command(about = "Kataan HTTP API server")]
struct Cli {
    #[arg(long)]
    vault: PathBuf,

    #[arg(long, default_value = "127.0.0.1:3001")]
    bind: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let state = AppState::new(cli.vault);
    let app = app(state);
    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .expect("bind server");
    println!("kataan-server listening on http://{}", cli.bind);
    axum::serve(listener, app).await.expect("serve");
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/vault", get(api::vault))
        .route("/api/folders", get(api::folders))
        .route("/api/folders/:folder", get(api::folder))
        .route("/api/documents/*id", get(api::document))
        .route("/api/validate", post(api::validate))
        .route("/api/rebuild-indexes", post(api::rebuild_indexes))
        .with_state(state)
}
