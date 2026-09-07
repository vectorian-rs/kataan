use std::path::PathBuf;

use argh::FromArgs;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod ignore;
mod state;
#[cfg(feature = "embed-ui")]
mod ui;
mod watch;

use state::AppState;

/// Kataan HTTP API server.
#[derive(Debug, FromArgs)]
struct Cli {
    /// vault to serve
    #[argh(option)]
    vault: PathBuf,

    /// address to bind (default 127.0.0.1:3001)
    #[argh(option, default = "String::from(\"127.0.0.1:3001\")")]
    bind: String,
}

#[tokio::main]
async fn main() {
    init_tracing();

    let cli: Cli = argh::from_env();
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
    info!(bind = %cli.bind, api = %format!("http://{}", cli.bind), "kataan-server API listening");
    #[cfg(feature = "embed-ui")]
    info!(url = %format!("http://{}", cli.bind), "embedded web UI available");
    #[cfg(not(feature = "embed-ui"))]
    info!(
        default_url = "http://127.0.0.1:3003",
        command = "bun run dev:web",
        "web UI is not embedded in this binary; run the web dev server separately"
    );
    axum::serve(listener, app).await.expect("serve");
}

fn init_tracing() {
    // Logs go to stderr (the conventional stream for diagnostics; still captured
    // by container/systemd log collectors), matching the CLI.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}
