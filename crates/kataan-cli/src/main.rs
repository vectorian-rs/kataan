use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const AGENT_GUIDE: &str = include_str!("../../../docs/kataan-agent-guide.md");

#[derive(Serialize)]
struct JsonReport {
    ok: bool,
    diagnostics: Vec<JsonDiagnostic>,
}

#[derive(Serialize)]
struct JsonDiagnostic {
    severity: String,
    code: String,
    message: String,
    path: Option<String>,
}

impl From<&kataan_core::diagnostic::Diagnostic> for JsonDiagnostic {
    fn from(diagnostic: &kataan_core::diagnostic::Diagnostic) -> Self {
        Self {
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            path: diagnostic.path.clone(),
        }
    }
}

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
        /// Emit the report as JSON on stdout instead of plain lines.
        #[arg(long)]
        json: bool,
    },
    RebuildIndexes {
        path: PathBuf,
    },
    /// The vault's model as JSON: types and their declared fields, edge
    /// predicates, and the type-level graph of what may connect to what.
    Ontology {
        path: PathBuf,
    },
    /// Graph queries over the vault, emitted as JSON on stdout.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    /// List or batch-fetch documents as JSON on stdout.
    Documents {
        path: PathBuf,
        #[command(flatten)]
        query: DocumentArgs,
    },
    Guide,
}

/// The `documents` filters, as command-line flags.
///
/// Deliberately field-for-field with `kataan_core::query::DocumentQuery`, and
/// converted by the `From` below — the same query, spelled for a shell, so the
/// CLI cannot quietly support a different set of filters from HTTP and MCP.
#[derive(Debug, clap::Args)]
pub struct DocumentArgs {
    /// Fetch these ids specifically (repeatable or comma-separated). Order is
    /// preserved and unknown ids come back in `missing`.
    #[arg(long = "id", value_delimiter = ',')]
    ids: Vec<String>,
    /// Restrict to a document type. Subtypes count.
    #[arg(long = "type")]
    r#type: Option<String>,
    #[arg(long)]
    status: Option<String>,
    /// Documents carrying every one of these labels.
    #[arg(long = "label", value_delimiter = ',')]
    labels: Vec<String>,
    /// Documents whose id is this folder or below it.
    #[arg(long)]
    path_prefix: Option<String>,
    /// Restrict to documents with an edge to this id.
    #[arg(long)]
    linked_to: Option<String>,
    /// With --linked-to: restrict to one predicate.
    #[arg(long)]
    predicate: Option<String>,
    /// With --linked-to: which direction to follow.
    #[arg(long, default_value = "both")]
    direction: DirectionArg,
    /// Only documents whose occurred_at is on or after this RFC 3339 bound.
    /// Inclusive, and compared at the bound's own precision, so a bare day
    /// covers the whole day.
    #[arg(long)]
    after: Option<String>,
    /// Only documents whose occurred_at is on or before this bound.
    #[arg(long)]
    before: Option<String>,
    /// Sort by: id, occurred-at, created-at, updated-at.
    #[arg(long, default_value = "id")]
    order: OrderArg,
    /// Reverse the sort. With --order updated-at, "what changed most recently".
    #[arg(long)]
    desc: bool,
    /// How much of each document to return. `full` adds declared fields,
    /// timestamps and edges for free; `markdown` adds the body, at one file
    /// read per document.
    #[arg(long, default_value = "metadata")]
    include: IncludeArg,
    /// Page size, at most 1000. Omitting it errors rather than truncating when
    /// more than 100 documents match.
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

impl From<DocumentArgs> for kataan_core::query::DocumentQuery {
    fn from(args: DocumentArgs) -> Self {
        Self {
            ids: args.ids.into(),
            r#type: args.r#type,
            status: args.status,
            labels: args.labels.into(),
            path_prefix: args.path_prefix,
            linked_to: args.linked_to,
            predicate: args.predicate,
            direction: args.direction.into(),
            after: args.after,
            before: args.before,
            order: args.order.into(),
            desc: args.desc,
            include: args.include.into(),
            limit: args.limit,
            offset: args.offset,
        }
    }
}

/// `--direction` and `--include` values, so `--help` lists them and shell
/// completion offers them.
///
/// Mirrors rather than derives `ValueEnum` on the core types: that would put
/// clap in `kataan-core`, which no library consumer of it should have to build.
/// The `From` impls are exhaustive, so a new variant fails to compile here.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum DirectionArg {
    Out,
    In,
    Both,
}

impl From<DirectionArg> for kataan_core::query::Direction {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Out => Self::Out,
            DirectionArg::In => Self::In,
            DirectionArg::Both => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum IncludeArg {
    Metadata,
    Full,
    Markdown,
}

impl From<IncludeArg> for kataan_core::query::Include {
    fn from(value: IncludeArg) -> Self {
        match value {
            IncludeArg::Metadata => Self::Metadata,
            IncludeArg::Full => Self::Full,
            IncludeArg::Markdown => Self::Markdown,
        }
    }
}

/// `--order` values, kebab-cased for the command line.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OrderArg {
    Id,
    OccurredAt,
    CreatedAt,
    UpdatedAt,
}

impl From<OrderArg> for kataan_core::query::Order {
    fn from(value: OrderArg) -> Self {
        match value {
            OrderArg::Id => Self::Id,
            OrderArg::OccurredAt => Self::OccurredAt,
            OrderArg::CreatedAt => Self::CreatedAt,
            OrderArg::UpdatedAt => Self::UpdatedAt,
        }
    }
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    /// Export nodes and links. Output is deterministic, so it diffs cleanly
    /// across runs and can be committed as a build artifact.
    Export {
        path: PathBuf,
        /// Restrict to these document types (repeatable or comma-separated).
        #[arg(long = "type", value_delimiter = ',')]
        types: Vec<String>,
        /// Restrict to these edge predicates (repeatable or comma-separated).
        #[arg(long = "predicate", value_delimiter = ',')]
        predicates: Vec<String>,
        /// Refuse rather than export more than this many nodes.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show what a document is connected to, in either or both directions.
    Neighbors {
        path: PathBuf,
        /// Canonical id, e.g. topics/rust.
        id: String,
        /// Restrict to one predicate.
        #[arg(long)]
        predicate: Option<String>,
        #[arg(long, default_value = "both")]
        direction: DirectionArg,
    },
}

/// Print a line to stdout, treating a closed pipe as a clean exit.
///
/// `println!` panics when the reader goes away, so `graph export | head` — the
/// obvious way to look at these commands' output — aborted with exit 101.
fn print_line(text: &str) -> Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{text}") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(error) => Err(error.into()),
    }
}

/// Exit code for a command that could not run at all — a missing path, an
/// unreadable vault, an I/O failure.
///
/// Distinct from the `1` that `validate` uses for "ran fine, found problems".
/// Both were `1`, so a CI script could not tell "this vault has diagnostics"
/// from "you gave me the wrong path" without parsing stderr. The split follows
/// the usual linter convention, and leaves the common scripted case — a
/// non-zero exit meaning the vault is invalid — on the code it already had.
const EXIT_OPERATIONAL_FAILURE: i32 = 2;

fn main() {
    init_tracing();
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(EXIT_OPERATIONAL_FAILURE);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { path, name } => {
            kataan_core::init::init_vault(&path, &name)?;
            info!(path = %path.display(), "initialized vault");
        }
        Command::Validate { path, json } => {
            // Diagnostics are this command's result: print them to stdout (plain
            // lines, or JSON with --json), keep operational logs on stderr, and
            // signal validity via the exit code.
            let report = kataan_core::validate::validate(path)?;
            let ok = report.is_ok();
            if json {
                let out = JsonReport {
                    ok,
                    diagnostics: report
                        .diagnostics
                        .iter()
                        .map(JsonDiagnostic::from)
                        .collect(),
                };
                print_line(&serde_json::to_string_pretty(&out)?)?;
            } else if ok {
                print_line("valid")?;
            } else {
                for issue in &report.diagnostics {
                    let severity = format!("{:?}", issue.severity).to_lowercase();
                    match issue.path.as_deref() {
                        Some(location) => print_line(&format!(
                            "{severity} [{}] {location}: {}",
                            issue.code, issue.message
                        ))?,
                        None => {
                            print_line(&format!("{severity} [{}]: {}", issue.code, issue.message))?
                        }
                    }
                }
            }
            if !ok {
                std::process::exit(1);
            }
        }
        Command::RebuildIndexes { path } => {
            kataan_core::rebuild::rebuild_indexes(&path)?;
            info!(path = %path.display(), "rebuilt indexes");
        }
        Command::Ontology { path } => {
            let vault = kataan_core::vault::LoadedVault::load(&path)?;
            let response = kataan_core::schema::ontology_response(&vault);
            print_line(&serde_json::to_string_pretty(&response)?)?;
        }
        Command::Graph { command } => match command {
            GraphCommand::Export {
                path,
                types,
                predicates,
                limit,
            } => {
                let vault = kataan_core::vault::LoadedVault::load(&path)?;
                let graph = kataan_core::query::subgraph(&vault, &types, &predicates, limit)?;
                print_line(&serde_json::to_string_pretty(&graph)?)?;
            }
            GraphCommand::Neighbors {
                path,
                id,
                predicate,
                direction,
            } => {
                let vault = kataan_core::vault::LoadedVault::load(&path)?;
                let id = kataan_core::id::CanonicalId::parse(&id)?;
                let result = kataan_core::query::neighbors(
                    &vault,
                    &id,
                    predicate.as_deref(),
                    direction.into(),
                )?;
                print_line(&serde_json::to_string_pretty(&result)?)?;
            }
        },
        Command::Documents { path, query } => {
            let vault = kataan_core::vault::LoadedVault::load(&path)?;
            let page = kataan_core::query::documents(&vault, &query.into())?;
            print_line(&serde_json::to_string_pretty(&page)?)?;
        }
        Command::Guide => {
            print_line(AGENT_GUIDE)?;
        }
    }

    Ok(())
}

fn init_tracing() {
    // Logs go to stderr so stdout carries only command output (validate results,
    // the guide, etc.), keeping the CLI scriptable.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}
