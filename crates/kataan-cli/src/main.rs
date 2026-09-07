use std::path::PathBuf;

use anyhow::Result;
use argh::{FromArgValue, FromArgs};
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

/// Filesystem-native Markdown/TOML knowledge workspace.
#[derive(Debug, FromArgs)]
struct Cli {
    #[argh(subcommand)]
    command: Command,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
enum Command {
    Init(InitArgs),
    Validate(ValidateArgs),
    RebuildIndexes(RebuildIndexesArgs),
    Ontology(OntologyArgs),
    Graph(GraphArgs),
    Documents(DocumentsArgs),
    Guide(GuideArgs),
}

/// Create a new vault.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "init")]
struct InitArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
    /// human name for the vault
    #[argh(option)]
    name: String,
}

/// Check the vault against its own model and report what is wrong.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "validate")]
struct ValidateArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
    /// emit the report as JSON on stdout instead of plain lines
    #[argh(switch)]
    json: bool,
}

/// Regenerate folder indexes and checksums.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "rebuild-indexes")]
struct RebuildIndexesArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
}

/// The vault's model as JSON: types and their declared fields, edge predicates,
/// and the type-level graph of what may connect to what.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "ontology")]
struct OntologyArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
}

/// Graph queries over the vault, emitted as JSON on stdout.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "graph")]
struct GraphArgs {
    #[argh(subcommand)]
    command: GraphCommand,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
enum GraphCommand {
    Export(GraphExportArgs),
    Neighbors(GraphNeighborsArgs),
}

/// Export nodes and links. Output is deterministic, so it diffs cleanly across
/// runs and can be committed as a build artifact.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "export")]
struct GraphExportArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
    /// restrict to these document types (repeatable or comma-separated)
    #[argh(option, long = "type")]
    types: Vec<String>,
    /// restrict to these edge predicates (repeatable or comma-separated)
    #[argh(option, long = "predicate")]
    predicates: Vec<String>,
    /// refuse rather than export more than this many nodes
    #[argh(option)]
    limit: Option<usize>,
}

/// Show what a document is connected to, in either or both directions.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "neighbors")]
struct GraphNeighborsArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
    /// canonical id, e.g. topics/rust
    #[argh(positional)]
    id: String,
    /// restrict to one predicate
    #[argh(option)]
    predicate: Option<String>,
    /// which direction to follow: out, in, both (default both)
    #[argh(option, default = "DirectionArg::Both")]
    direction: DirectionArg,
}

/// Print the agent guide.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "guide")]
struct GuideArgs {}

// Field-for-field with `kataan_core::query::DocumentQuery` and converted by the
// `From` below — the same query, spelled for a shell, so the CLI cannot quietly
// support a different set of filters from HTTP and MCP. Kept out of the doc
// comment because argh prints that verbatim as the command's help.
/// List or batch-fetch documents as JSON on stdout.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "documents")]
struct DocumentsArgs {
    /// vault path
    #[argh(positional)]
    path: PathBuf,
    /// fetch these ids specifically (repeatable or comma-separated); order is
    /// preserved and unknown ids come back in `missing`
    #[argh(option, long = "id")]
    ids: Vec<String>,
    /// restrict to a document type; subtypes count
    #[argh(option, long = "type")]
    r#type: Option<String>,
    /// restrict to a status
    #[argh(option)]
    status: Option<String>,
    /// documents carrying every one of these labels (repeatable or
    /// comma-separated)
    #[argh(option, long = "label")]
    labels: Vec<String>,
    /// documents whose id is this folder or below it
    #[argh(option)]
    path_prefix: Option<String>,
    /// restrict to documents with an edge to this id
    #[argh(option)]
    linked_to: Option<String>,
    /// with --linked-to: restrict to one predicate
    #[argh(option)]
    predicate: Option<String>,
    /// with --linked-to: which direction to follow: out, in, both (default
    /// both)
    #[argh(option, default = "DirectionArg::Both")]
    direction: DirectionArg,
    /// only documents whose occurred_at is on or after this RFC 3339 bound;
    /// inclusive, and compared at the bound's own precision, so a bare day
    /// covers the whole day
    #[argh(option)]
    after: Option<String>,
    /// only documents whose occurred_at is on or before this bound
    #[argh(option)]
    before: Option<String>,
    /// sort by: id, occurred_at, created_at, updated_at (default id)
    #[argh(option, default = "OrderArg::Id")]
    order: OrderArg,
    /// reverse the sort; with --order updated_at, "what changed most recently"
    #[argh(switch)]
    desc: bool,
    /// how much of each document to return: metadata, full, markdown (default
    /// metadata). `full` adds declared fields, timestamps and edges for free;
    /// `markdown` adds the body, at one file read per document
    #[argh(option, default = "IncludeArg::Metadata")]
    include: IncludeArg,
    /// page size, at most 1000; omitting it errors rather than truncating when
    /// more than 100 documents match
    #[argh(option)]
    limit: Option<usize>,
    /// how many matches to skip; use with --limit to page
    #[argh(option, default = "0")]
    offset: usize,
}

impl From<DocumentsArgs> for kataan_core::query::DocumentQuery {
    fn from(args: DocumentsArgs) -> Self {
        Self {
            ids: split_lists(args.ids).into(),
            r#type: args.r#type,
            status: args.status,
            labels: split_lists(args.labels).into(),
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

/// Flatten `--label a,b --label c` into one list.
///
/// argh has no delimiter option, so the splitting is done here — and it is done
/// the same way `wire::Csv` does it for a URL query string, which is the point:
/// the same flag spelled either way reaches core as the same list.
fn split_lists(values: Vec<String>) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Command-line spellings of the core enums.
///
/// Mirrors rather than the core types themselves: `FromArgValue` is argh's
/// trait and `Direction` is kataan-core's, so the orphan rule forbids the impl
/// anywhere but here — deriving on the core types would mean putting an
/// argument parser inside the library. The `From` impls are exhaustive, so a
/// new variant fails to compile rather than silently going missing.
///
/// argh spells the values `occurred_at` rather than clap's `occurred-at`, which
/// is what HTTP and MCP already accept.
#[derive(Debug, Clone, Copy, FromArgValue)]
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

#[derive(Debug, Clone, Copy, FromArgValue)]
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

#[derive(Debug, Clone, Copy, FromArgValue)]
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
    let cli: Cli = argh::from_env();

    match cli.command {
        Command::Init(args) => {
            kataan_core::init::init_vault(&args.path, &args.name)?;
            info!(path = %args.path.display(), "initialized vault");
        }
        Command::Validate(args) => {
            // Diagnostics are this command's result: print them to stdout (plain
            // lines, or JSON with --json), keep operational logs on stderr, and
            // signal validity via the exit code.
            let report = kataan_core::validate::validate(args.path)?;
            let ok = report.is_ok();
            if args.json {
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
        Command::RebuildIndexes(args) => {
            kataan_core::rebuild::rebuild_indexes(&args.path)?;
            info!(path = %args.path.display(), "rebuilt indexes");
        }
        Command::Ontology(args) => {
            let vault = kataan_core::vault::LoadedVault::load(&args.path)?;
            let response = kataan_core::schema::ontology_response(&vault);
            print_line(&serde_json::to_string_pretty(&response)?)?;
        }
        Command::Graph(args) => match args.command {
            GraphCommand::Export(args) => {
                let vault = kataan_core::vault::LoadedVault::load(&args.path)?;
                let graph = kataan_core::query::subgraph(
                    &vault,
                    &split_lists(args.types),
                    &split_lists(args.predicates),
                    args.limit,
                )?;
                print_line(&serde_json::to_string_pretty(&graph)?)?;
            }
            GraphCommand::Neighbors(args) => {
                let vault = kataan_core::vault::LoadedVault::load(&args.path)?;
                let id = kataan_core::id::CanonicalId::parse(&args.id)?;
                let result = kataan_core::query::neighbors(
                    &vault,
                    &id,
                    args.predicate.as_deref(),
                    args.direction.into(),
                )?;
                print_line(&serde_json::to_string_pretty(&result)?)?;
            }
        },
        Command::Documents(args) => {
            let vault = kataan_core::vault::LoadedVault::load(&args.path)?;
            let page = kataan_core::query::documents(&vault, &args.into())?;
            print_line(&serde_json::to_string_pretty(&page)?)?;
        }
        Command::Guide(_) => {
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
