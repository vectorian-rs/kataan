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
