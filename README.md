# Kataan

Filesystem-native Markdown/TOML knowledge workspace.

## Layout

- `crates/kataan-core` — vault model, checksums, validation, indexes
- `crates/kataan-cli` — CLI entrypoint
- `crates/kataan-server` — Rust HTTP API
- `apps/web` — Astro frontend
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
