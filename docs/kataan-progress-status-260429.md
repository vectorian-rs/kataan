# Kataan Progress Status — 2026-04-29

## Executive summary

Kataan has a solid read/validate/rebuild core: canonical IDs, nested folder documents, metadata-only `LoadedVault`, ontology-backed edges, read-only server/UI, route locators, and the special `code/` tool folder are implemented and tested. The main unfinished areas are the full recursive Merkle/index model, live server safety behavior, watcher/single-writer write architecture, raw intake, MCP, and usable agent workflows.

## Overall status

| Area         | Status  | Notes |
| ------------ | ------- | ----- |
| Spec clarity | partial | Brief/plan are detailed and mostly current, but still contain some tension around folder indexes listing subfolders and every core folder having indexes while `code/` is intentionally exempt. |
| Core model   | partial | `CanonicalId`, `DocumentRecord`, `LoadedVault`, graph, route tokens, and `code/` exemption exist. Missing case-insensitive collision diagnostics and materialized `FacetIndex`. |
| Validation   | partial | Recursive folder/doc validation is broad and tested. Missing explicit case-insensitive collision validation and direct root `schema_version` semantic/version checks beyond TOML deserialization. |
| Rebuild      | partial | Rebuild updates direct document entries and checksums and skips `code/`. It still primarily iterates top-level type folders/direct files and does not fully rebuild recursive subfolder entries/checksums as specified. |
| Init         | done    | `kataan init` creates root config, ontology, core document folders, special `code/`, type definitions, and then rebuilds indexes. Init→validate→rebuild→validate succeeds. |
| Server       | partial | Server boots `Arc<RwLock<LoadedVault>>`, reads Markdown on demand, supports resolve/document/folder APIs, and reloads after rebuild. Missing read-only-on-error boot behavior, watcher, mpsc write queue, and write APIs. |
| UI           | partial | Read-only Astro UI supports folders, nested documents, metadata, validation/rebuild, Markdown rendering, table styling, and route-token reload URLs. Missing edit/intake/agent surfaces and robust failure states. |
| Agent        | partial | `crates/kataan-agent` skeleton exists, but no provider implementation, ask command, server endpoint, proposal application, or UI overlay is implemented. |
| Tests        | done    | Rust tests, clippy, web check, and CLI milestone commands pass in this audit run. Coverage is good for current core behavior, weaker for end-to-end server failure modes and recursive rebuild semantics. |

## Implemented

- Canonical ID model in `crates/kataan-core/src/id.rs`:
  - path-based vault-relative IDs without extensions
  - `/` normalization
  - folder index IDs use folder paths
  - mixed-case URL-safe segments are accepted
- Metadata-only vault loading in `crates/kataan-core/src/vault.rs`:
  - `DocumentRecord` stores metadata, paths, ancestors, facets, checksums, and folder marker
  - `LoadedVault` stores config, type registry, ontology, documents, route token map, and graph
  - Markdown is read on demand via `LoadedVault::read_markdown` / server file reads
- Special `code/` folder behavior:
  - constants in `crates/kataan-core/src/constants.rs`
  - skipped by loader in `crates/kataan-core/src/vault.rs`
  - skipped by validator in `crates/kataan-core/src/validate.rs`
  - skipped by rebuild in `crates/kataan-core/src/rebuild.rs`
  - handled by server folder API in `crates/kataan-server/src/api.rs`
- Recursive validation in `crates/kataan-core/src/validate.rs`:
  - missing folder `index.md` / `index.toml`
  - nested sidecar pairing
  - folder depth
  - type-folder mapping
  - status/actor values
  - Markdown checksum drift
  - index drift/checksum mismatch
  - ontology/edge predicate and target checks
- Ontology validation in `crates/kataan-core/src/ontology.rs`:
  - predicate `snake_case`
  - required `from`, `to`, `cardinality`
  - `inverse` xor `symmetric`
  - symmetric endpoint equality
  - cardinality enum checks
  - `*` polymorphic endpoint matching via `type_allowed`
- Graph model in `crates/kataan-core/src/graph.rs`:
  - keyed by canonical ID
  - path children
  - outgoing/incoming edges
  - inverse and symmetric adjacency derived from ontology
- Checksums and atomic writes:
  - raw-byte BLAKE3 helpers in `crates/kataan-core/src/checksum.rs`
  - same-dir tempfile + fsync + persist in `crates/kataan-core/src/write.rs`
- Init in `crates/kataan-core/src/init.rs`:
  - creates `kataan.toml`, `ontology.toml`, core folders, `code/`, type definitions, and runs rebuild
- Server read API:
  - `AppState { vault_path, vault: Arc<RwLock<LoadedVault>> }` in `crates/kataan-server/src/state.rs`
  - `/api/vault`, `/api/folders`, `/api/folder`, `/api/document`, `/api/resolve`, `/api/validate`, `/api/rebuild-indexes` in `crates/kataan-server/src/api.rs`
  - rebuild reloads `LoadedVault`
- UI route locators:
  - token generation in `crates/kataan-core/src/vault.rs::route_token_for_id`
  - API resolver in `crates/kataan-server/src/api.rs::resolve_route`
  - frontend restore/update logic in `apps/web/src/lib/dashboard.ts`
  - dynamic Astro route in `apps/web/src/pages/[type]/[token].astro`
- Read-only UI:
  - API client in `apps/web/src/lib/api.ts`
  - dashboard behavior in `apps/web/src/lib/dashboard.ts`
  - layout/components/styles under `apps/web/src/`

## Partially implemented

- `LoadedVault` facets: `DocumentRecord.facets` exists, but no global `FacetIndex` with `by_facet`, `by_label`, `by_ancestor`, `by_type`, and `by_status` exists.
- Rebuild recursion: checksum helpers support recursive subfolder inputs, but `crates/kataan-core/src/rebuild.rs::rebuild_indexes` still walks each top-level type folder's direct Markdown files and writes only direct `[[documents]]`; it does not fully rebuild recursive subfolder entries/checksums as described in the brief/plan.
- Folder indexes: `FolderIndex` in `crates/kataan-core/src/index.rs` has `documents` but no explicit subfolder entries, while the spec says indexes list direct child documents and subfolders.
- Server boot safety: server loads `LoadedVault` at boot and fails hard on load errors; it does not implement the specified degraded read-only-on-error mode with diagnostics + rebuild.
- Server live state: shared `Arc<RwLock<LoadedVault>>` exists, but there is no debounced watcher, minimal patch path, command queue, or write serialization layer.
- Agent crate: structure exists in `crates/kataan-agent`, but providers, commands, server endpoint, proposal review/apply, and UI overlay are not implemented.
- URL locators: implemented for current documents, but no collision diagnostics for route-token truncation are implemented.

## Missing

- Case-insensitive filename/canonical-ID collision validation for cross-platform safety.
- Materialized `FacetIndex` and query/filter APIs.
- Filesystem watcher with debounce, minimal metadata updates, and full reload fallback.
- Single-writer mpsc command queue for mutating server APIs.
- Base-content-hash conflict checking for write/proposal application.
- Manual raw intake endpoint/UI.
- Edge mutation API (`add_edge`, `remove_edge`, `replace_edges_for_predicate`).
- MCP read/repair surface.
- OpenAI/Anthropic provider implementations and ask commands.
- Server `/api/agent` endpoint and UI agent overlay.
- Supply-chain audit beyond successful dependency resolution/builds.

## Drift from spec

- `docs/kataan-brief.md` says folder indexes list direct child documents and subfolders, and recursive Merkle checksums include sorted subfolder checksums. Implementation has checksum helper support, but `FolderIndex` lacks subfolder records and rebuild does not fully write recursive subfolder entries.
- `docs/kataan-plan.md` says `rebuild-indexes` updates root `updated_at`; current `crates/kataan-core/src/rebuild.rs` does not update root `kataan.toml` timestamps.
- Brief/plan specify read-only-on-error server boot; `crates/kataan-server/src/main.rs` currently `expect("load vault")` and exits if `LoadedVault::load` fails.
- Brief/plan specify watcher behavior; no watcher implementation or `notify` usage is present.
- Brief/plan specify single-writer command queue; no mpsc queue exists in server code.
- Brief says validation should detect case-insensitive collisions; no such diagnostic is implemented.
- Brief core mapping table omits `code`, while the vault index example and later raw-vs-organized section include it.

## Test and command evidence

Commands run during this audit:

```sh
cargo test
```

Summary: passed. `kataan-core` ran 44 tests, `kataan-server` ran 8 tests, agent/CLI crates had no unit tests, doc tests passed.

```sh
cargo clippy --all-targets -- -D warnings
```

Summary: passed with no warnings.

```sh
bun --filter @kataan/web check
```

Summary: passed. Astro check reported 14 files, 0 errors, 0 warnings, 0 hints.

```sh
tmp=$(mktemp -d)
cargo run -p kataan-cli -- init "$tmp/vault" --name "Audit Vault"
cargo run -p kataan-cli -- validate "$tmp/vault"
cargo run -p kataan-cli -- rebuild-indexes "$tmp/vault"
cargo run -p kataan-cli -- validate "$tmp/vault"
```

Summary: exit 0. Output included `initialized vault ...`, `valid`, `rebuilt indexes ...`, `valid`.

Repository state at audit start was not clean:

```txt
 D docs/kataan-implementation-progress.md
?? docs/kataan-progress-prompt.md
?? docs/kataan-verification-prompt.md
```

## Highest-risk invariant

The highest-risk invariant is recursive folder index/Merkle consistency: validation and loaders understand nested folders, but rebuild does not fully materialize recursive subfolder entries/checksums, so manual nested edits plus rebuild can leave the stored index model weaker than the spec requires.

## Recommended next actions

1. Implement full recursive `rebuild-indexes`: post-order walk, explicit subfolder entries in `FolderIndex`, recursive folder checksums, and root `updated_at` update.
2. Add validation and tests for case-insensitive canonical-ID collisions and route-token collisions.
3. Implement `FacetIndex` on `LoadedVault` and expose query/filter helpers before adding richer UI filters or agent context selection.
4. Change server boot to serve diagnostics + rebuild in read-only mode when validation/load fails instead of exiting.
5. Add watcher/reload design in code: debounce events, patch clear metadata changes, full reload on structural ambiguity.
6. Add single-writer mutation queue and base-hash conflict checking before implementing raw intake, edge mutation, or agent proposal apply.
7. Update docs to consistently include `code` in all core type/folder mapping tables and clarify that `code/` is exempt from folder index/Merkle semantics.
