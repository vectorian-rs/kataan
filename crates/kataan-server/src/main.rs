use std::path::PathBuf;

use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod ignore;
mod state;
mod watch;

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
    init_tracing();

    let cli = Cli::parse();
    let state = match AppState::new(cli.vault) {
        Ok(state) => state,
        Err(error) => {
            error!(error = %error, "failed to load vault");
            std::process::exit(1);
        }
    };
    info!("loaded vault successfully");
    watch::spawn_watcher(state.clone());

    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .expect("bind server");
    info!(bind = %cli.bind, "kataan-server listening");
    axum::serve(listener, app).await.expect("serve");
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
}
