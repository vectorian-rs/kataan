# Kataan

[![CI](https://github.com/vectorian-rs/kataan/actions/workflows/ci.yml/badge.svg)](https://github.com/vectorian-rs/kataan/actions/workflows/ci.yml)

Filesystem-native Markdown/TOML knowledge workspace.

## Layout

- `crates/kataan-core` — vault model, checksums, validation, indexes
- `crates/kataan-cli` — CLI entrypoint
- `crates/kataan-server` — Rust HTTP API
- `apps/web` — Astro frontend (static SPA; can be embedded into the server)
- `packages/client` — shared TypeScript client/types
- `examples/vault` — example vault

## First commands

```sh
mise install
bun install
cargo run -p kataan-cli -- validate examples/vault
cargo run -p kataan-server -- --vault examples/vault
bun run dev:web
```

The web app runs on `http://127.0.0.1:3000` and proxies `/api` to the Rust backend at `http://127.0.0.1:3001`. If port `3000` is already in use, Astro exits instead of silently switching ports.

To use a different backend port:

```sh
KATAAN_API_PROXY_TARGET=http://127.0.0.1:3002 bun run dev:web
```

To use a different web port:

```sh
KATAAN_WEB_PORT=3003 bun run dev:web
```

Both can be combined:

```sh
KATAAN_WEB_PORT=3003 KATAAN_API_PROXY_TARGET=http://127.0.0.1:3002 bun run dev:web
```

## Single binary (embedded UI)

`apps/web` is a client-routed SPA that talks to the server over `/api`, so the
static build can be embedded directly into `kataan-server`. Built with the
`embed-ui` feature, one binary serves both the API and the UI on a single port —
no separate web process, and nothing run from the repo at runtime.

```sh
# Build the UI, then install the server with the UI embedded into ~/.cargo/bin
mise run install-server

# Run it — open http://127.0.0.1:3001
kataan-server --vault /path/to/vault
```

`mise run build-app` produces the same binary at `target/release/kataan-server`
without installing. The feature is off by default, so `cargo build`/`test`/
`clippy` don't require a prior web build. In debug builds the assets are read
from `apps/web/dist` at runtime; release builds bake them into the binary. For
UI development keep using `bun run dev:web` (hot reload, proxying to the API).
