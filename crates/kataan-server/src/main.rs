use std::path::PathBuf;

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
    let state = AppState::new(cli.vault).expect("load vault");
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .expect("bind server");
    println!("kataan-server listening on http://{}", cli.bind);
    axum::serve(listener, app).await.expect("serve");
}
