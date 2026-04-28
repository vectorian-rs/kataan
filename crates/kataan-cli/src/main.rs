use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "kataan")]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { path, name } => {
            kataan_core::init::init_vault(&path, &name)?;
            println!("initialized vault at {}", path.display());
        }
        Command::Validate { path } => {
            let report = kataan_core::validate::validate(path)?;
            if report.is_ok() {
                println!("valid");
            } else {
                for issue in report.diagnostics {
                    if let Some(path) = issue.path {
                        println!(
                            "{:?} [{}] {}: {}",
                            issue.severity, issue.code, path, issue.message
                        );
                    } else {
                        println!("{:?} [{}]: {}", issue.severity, issue.code, issue.message);
                    }
                }
                std::process::exit(1);
            }
        }
        Command::RebuildIndexes { path } => {
            println!("rebuild-indexes not implemented yet: {}", path.display());
        }
    }

    Ok(())
}
