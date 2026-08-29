# Kataan

[![CI](https://github.com/vectorian-rs/kataan/actions/workflows/ci.yml/badge.svg)](https://github.com/vectorian-rs/kataan/actions/workflows/ci.yml)

Filesystem-native Markdown/TOML knowledge workspace.

## Layout

- `crates/kataan-core` — vault model, checksums, validation, indexes
- `crates/kataan-cli` — CLI entrypoint
- `crates/kataan-server` — Rust HTTP API
- `crates/kataan-mcp` — MCP server (agents read + write the vault over stdio)
- `apps/web` — Astro frontend (static SPA; can be embedded into the server)
- `packages/client` — shared TypeScript client/types
- `examples/vault` — example vault

## First commands

```sh
mise install
bun install
bun --filter @kataan/web build
cargo run -p kataan-cli -- validate examples/vault
cargo run -p kataan-server -- --vault examples/vault
```

The default `kataan-server` build embeds the web UI. Open
`http://127.0.0.1:3001` to use the app; the API is under `/api`, for example
`http://127.0.0.1:3001/api/health`.

For UI development with hot reload, keep the server running and start Astro in a
second terminal:

```sh
bun run dev:web
```

The dev web app runs on `http://127.0.0.1:3003` (set by `KATAAN_WEB_PORT` in
`mise.toml`; Astro defaults to `3000` when it is unset) and proxies `/api` to the
Rust backend at `http://127.0.0.1:3001`. If the web port is already in use, Astro
exits instead of silently switching ports.

To use a different backend port:

```sh
KATAAN_API_PROXY_TARGET=http://127.0.0.1:3002 bun run dev:web
```

To use a different web port:

```sh
KATAAN_WEB_PORT=3005 bun run dev:web
```

Both can be combined:

```sh
KATAAN_WEB_PORT=3005 KATAAN_API_PROXY_TARGET=http://127.0.0.1:3002 bun run dev:web
```

## Single binary (embedded UI)

`apps/web` is a client-routed SPA that talks to the server over `/api`, so the
static build is embedded directly into `kataan-server` by default. One binary
serves both the API and the UI on a single port — no separate web process, and
nothing run from the repo at runtime.

```sh
# Build the UI, then install the server with the UI embedded into ~/.cargo/bin
mise run install-server

# Run it — open http://127.0.0.1:3001
kataan-server --vault /path/to/vault
```

`mise run build-app` produces the same binary at `target/release/kataan-server`
without installing. In debug builds the assets are read from `apps/web/dist` at
runtime; release builds bake them into the binary. For an API-only server, build
or run with `--no-default-features`.

## Use with an MCP client (agents)

`kataan-mcp` exposes a vault to any [Model Context Protocol](https://modelcontextprotocol.io)
client (Claude Desktop, IDE agents) as typed tools, so an agent can both **read**
and **write** the vault. It speaks MCP directly over stdio — no extra runtime.

```sh
cargo install --path crates/kataan-mcp   # installs `kataan-mcp` into ~/.cargo/bin
```

Register it with your client. For Claude Desktop, add to
`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "kataan": {
      "command": "kataan-mcp",
      "args": ["--vault", "/path/to/vault"]
    }
  }
}
```

Tools exposed by `kataan-mcp`:

| Tool | Kind | Arguments | Description |
| --- | --- | --- | --- |
| `search` | read | `q?`, `kind?`, `type?`, `status?`, `facet?`, `path_prefix?`, `limit?`, `offset?` | Full-text keyword search across the vault. All filters are optional. |
| `get_document` | read | `id` | Fetch one document's metadata, Markdown body, ancestors, and facets by canonical id, e.g. `notes/my-note`. |
| `list_folders` | read | none | Return the vault's type-to-folder mapping. |
| `get_folder` | read | `id` | List direct child folders and documents under a folder id, e.g. `notes`. |
| `resolve` | read | `type`, `token` | Resolve an alias or slug route token within a type folder to a canonical id. |
| `schema` | read | `kind` | Return the TOML schema for a kind such as `document`, `ontology`, or `index`. |
| `vault_info` | read | none | Return the vault configuration/index. |
| `resolve_path` | read | `path` | Resolve a filesystem path to a canonical id. Accepts either file of a pair (`notes/x.md`, `notes/x.toml`), a folder's `index`, or the extensionless form. |
| `documents` | read | `ids?`, `type?`, `status?`, `labels?`, `path_prefix?`, `linked_to?`, `predicate?`, `direction?`, `include?`, `limit?`, `offset?` | List or batch-fetch documents in one call. Metadata only unless `include: "markdown"`. Matching more than `limit` is an error, not a truncation. |
| `neighbors` | read | `id`, `predicate?`, `direction?` | What a document is connected to, grouped by predicate and hydrated with each neighbour's type/title/status. Incoming edges use the ontology's inverse predicate. |
| `subgraph` | read | `types?`, `predicates?` | Export `{nodes, links}` for the vault. Each edge appears once, in the direction it was authored. |
| `create_document` | write | `type`, `title`, `body`, `parent?`, `aliases?`, `labels?`, `status?`, `occurred_at?`, `fields?` | Create a new document and return its canonical id. `fields` writes extra top-level sidecar keys. |
| `update_document` | write | `id`, `body?`, `status?`, `aliases?`, `labels?`, `occurred_at?` | Update an existing document's body and/or metadata. Omitted fields are left unchanged. |
| `add_edge` | write | `source`, `predicate`, `target` | Add an ontology-validated edge from one document to another. |

`neighbors` answers questions `get_document` cannot: it returns raw outgoing edges
only, so "who works at this organization" is unanswerable from it — that edge is
declared on each person, and the inverse exists only in the graph. Prefer
`neighbors` for one document and `subgraph` for a whole graph.

All tool results are JSON. Writes go through the validated mutation layer, so every
change is well-formed (correct id, sidecar, checksum, folder indexes,
ontology-legal edges) and the search index is refreshed. An illegal change (e.g.
an edge your ontology forbids) is rejected with an error instead of corrupting the
vault.

stdout carries only the JSON-RPC protocol; logs go to stderr (`RUST_LOG` controls
verbosity).
