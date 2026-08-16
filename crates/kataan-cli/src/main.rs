use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use kataan_core::diagnostic::Severity;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const AGENT_GUIDE: &str = include_str!("../../../docs/kataan-agent-guide.md");

#[derive(Debug, Parser)]
#[command(name = "kataan-cli")]
#[command(about = "Filesystem-native Markdown/TOML knowledge workspace")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        path: PathBuf,
        #[arg(long)]
        name: String,
    },
    Validate {
        path: PathBuf,
    },
    RebuildIndexes {
        path: PathBuf,
    },
    #[command(alias = "quide")]
    Guide,
}

fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();

    match cli.command {
        Command::Init { path, name } => {
            kataan_core::init::init_vault(&path, &name)?;
            info!(path = %path.display(), "initialized vault");
        }
        Command::Validate { path } => {
            let report = kataan_core::validate::validate(path)?;
            if report.is_ok() {
                info!("vault is valid");
            } else {
                for issue in &report.diagnostics {
                    let path = issue.path.as_deref().unwrap_or("-");
                    match issue.severity {
                        Severity::Error => {
                            error!(code = %issue.code, path, "{}", issue.message)
                        }
                        Severity::Warning => {
                            warn!(code = %issue.code, path, "{}", issue.message)
                        }
                        Severity::Info => {
                            info!(code = %issue.code, path, "{}", issue.message)
                        }
                    }
                }
                std::process::exit(1);
            }
        }
        Command::RebuildIndexes { path } => {
            kataan_core::rebuild::rebuild_indexes(&path)?;
            info!(path = %path.display(), "rebuilt indexes");
        }
        Command::Guide => {
            print!("{AGENT_GUIDE}");
        }
    }

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
}
