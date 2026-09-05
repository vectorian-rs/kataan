//! `kataan-mcp` — a Model Context Protocol server exposing a kataan vault to LLM
//! clients (Claude Desktop, IDE agents) as tools. It speaks MCP directly:
//! newline-delimited JSON-RPC 2.0 over stdio, no SDK — the protocol surface is
//! small and stable, which keeps this crate lean and free of a churning
//! dependency. stdout is the JSON-RPC channel; all logs go to stderr.

use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use anyhow::Result;
use clap::Parser;
use serde_json::{json, Value};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod tools;

/// The MCP protocol revision we default to when a client doesn't request one.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "kataan-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol revisions this server implements. An `initialize` naming one of
/// these is answered with it; anything else is answered with the default, and
/// the client decides whether it can proceed.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

#[derive(Debug, Parser)]
#[command(name = "kataan-mcp")]
#[command(about = "MCP server for a kataan vault (read + write over stdio)")]
struct Cli {
    /// Path to the vault to serve.
    #[arg(long)]
    vault: PathBuf,
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    // Build the search index up front so `search` works from the first call.
    if let Err(error) = tools::reindex_search(&cli.vault) {
        tracing::warn!(error = %error, "initial search reindex failed; search may be empty");
    }
    tracing::info!(vault = %cli.vault.display(), "kataan-mcp ready");

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        // A malformed line is that line's problem: reporting a parse error and
        // reading on keeps one corrupt byte from ending the whole session.
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                let response = error_response(Value::Null, -32700, format!("parse error: {error}"));
                serde_json::to_writer(&mut stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&cli.vault, &message),
            Err(error) => Some(error_response(
                Value::Null,
                -32700,
                format!("parse error: {error}"),
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Dispatch one JSON-RPC message. Returns `None` for notifications (no `id`),
/// which get no reply.
fn handle_message(vault: &std::path::Path, message: &Value) -> Option<Value> {
    // Batching was removed in the revision we default to, but older clients may
    // still send an array. Answering keeps them from hanging on a reply that
    // would otherwise never come.
    if !message.is_object() {
        return Some(error_response(
            Value::Null,
            -32600,
            "invalid request: expected a single JSON-RPC object (batches are not supported)"
                .to_owned(),
        ));
    }
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            // Reply with a version we actually implement, not whatever was
            // asked for. Echoing the request claimed fluency in every revision
            // of the protocol — including ones whose batching and content
            // rules this server does not follow — and left the client no way to
            // detect the mismatch and disconnect, which is what the handshake
            // is for.
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let protocol_version = requested
                .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);
            Some(ok_response(
                id?,
                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                }),
            ))
        }
        "ping" => Some(ok_response(id?, json!({}))),
        "tools/list" => Some(ok_response(id?, json!({ "tools": tools::list() }))),
        "tools/call" => {
            let id = id?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            // Per MCP, a tool that fails returns a result with isError=true (so
            // the model sees the message), not a JSON-RPC protocol error.
            Some(match tools::call(vault, name, &arguments) {
                Ok(text) => ok_response(id, tool_content(&text, false)),
                Err(error) => ok_response(id, tool_content(&error.to_string(), true)),
            })
        }
        // notifications/initialized, notifications/cancelled, etc. — no reply.
        _ if id.is_none() => None,
        _ => Some(error_response(
            id.unwrap(),
            -32601,
            format!("method not found: {method}"),
        )),
    }
}

fn tool_content(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn init_tracing() {
    // stdout is the JSON-RPC channel; logs MUST go to stderr or they corrupt it.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // A path that never has to be read: the messages under test don't touch the
    // vault (initialize, tools/list, notifications, unknown methods, and a
    // tools/call that fails on tool lookup before any I/O).
    fn no_vault() -> &'static Path {
        Path::new("/nonexistent")
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    #[test]
    fn initialize_echoes_protocol_version_and_advertises_tools() {
        let message = request(1, "initialize", json!({ "protocolVersion": "2025-03-26" }));
        let response = handle_message(no_vault(), &message).expect("initialize replies");

        let result = &response["result"];
        assert_eq!(result["protocolVersion"], "2025-03-26");
        assert_eq!(result["capabilities"]["tools"], json!({}));
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(response["id"], 1);
    }

    /// The handshake exists so a client can detect a mismatch and disconnect.
    /// Echoing whatever was asked for claimed fluency in every revision of the
    /// protocol, including ones this server does not implement.
    #[test]
    fn initialize_answers_with_a_version_it_actually_supports() {
        for supported in SUPPORTED_PROTOCOL_VERSIONS {
            let message = request(1, "initialize", json!({ "protocolVersion": supported }));
            let response = handle_message(no_vault(), &message).unwrap();
            assert_eq!(response["result"]["protocolVersion"], *supported);
        }

        for unsupported in ["1999-01-01", "2099-12-31", "nonsense"] {
            let message = request(1, "initialize", json!({ "protocolVersion": unsupported }));
            let response = handle_message(no_vault(), &message).unwrap();
            assert_eq!(
                response["result"]["protocolVersion"], DEFAULT_PROTOCOL_VERSION,
                "`{unsupported}` was echoed back as though it were supported"
            );
        }
    }

    #[test]
    fn initialize_falls_back_to_default_protocol_version() {
        let message = request(1, "initialize", json!({}));
        let response = handle_message(no_vault(), &message).unwrap();
        assert_eq!(
            response["result"]["protocolVersion"],
            DEFAULT_PROTOCOL_VERSION
        );
    }

    #[test]
    fn notifications_get_no_reply() {
        let message = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(no_vault(), &message).is_none());
    }

    #[test]
    fn tools_list_returns_the_full_catalogue() {
        let message = request(2, "tools/list", Value::Null);
        let response = handle_message(no_vault(), &message).unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 16);
        for expected in [
            "create_document",
            "neighbors",
            "subgraph",
            "resolve_path",
            "documents",
            "remove_edge",
            "replace_edges_for_predicate",
            "ontology",
        ] {
            assert!(
                tools.iter().any(|tool| tool["name"] == expected),
                "`{expected}` missing from the catalogue"
            );
        }
    }

    #[test]
    fn ping_replies_empty() {
        let response = handle_message(no_vault(), &request(3, "ping", Value::Null)).unwrap();
        assert_eq!(response["result"], json!({}));
    }

    #[test]
    fn unknown_method_is_a_jsonrpc_error() {
        let response =
            handle_message(no_vault(), &request(4, "does/not/exist", Value::Null)).unwrap();
        assert_eq!(response["error"]["code"], -32601);
        assert!(response.get("result").is_none());
    }

    #[test]
    fn unknown_method_as_notification_is_silent() {
        let message = json!({ "jsonrpc": "2.0", "method": "does/not/exist" });
        assert!(handle_message(no_vault(), &message).is_none());
    }

    #[test]
    fn failing_tool_call_is_an_iserror_result_not_a_protocol_error() {
        // An unknown tool fails inside tools::call before any vault access, so
        // this exercises the isError path without needing a real vault.
        let message = request(
            5,
            "tools/call",
            json!({ "name": "no_such_tool", "arguments": {} }),
        );
        let response = handle_message(no_vault(), &message).unwrap();
        assert!(
            response.get("error").is_none(),
            "tool failures are results, not protocol errors"
        );
        assert_eq!(response["result"]["isError"], true);
    }
}
