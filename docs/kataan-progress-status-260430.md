# Kataan Progress Status — 2026-04-30

## Executive summary

Kataan is now a functional read-only filesystem-native vault browser with metadata-only loading, registry-driven custom document types, ontology-backed edges, document/file split UI, syntax-highlighted file previews, schema guidance, validation, init, and partial rebuild support. The main gaps remain recursive rebuild/Merkle completeness, watcher/live reload, serialized write queue and mutation APIs, manual intake, and real agent proposal/provider flow.

## Overall status

| Area         | Status  | Notes |
| ------------ | ------- | ----- |
| Spec clarity | done    | Brief/plan now describe `intake`, registry-driven custom types, files vs documents, and highlight API. |
| Core model   | partial | Canonical IDs, metadata-only `LoadedVault`, type registry, graph, and file/document model exist; no global `FacetIndex` yet. |
| Validation   | partial | Many invariants are checked, including custom types and ontology edges; collision checks and full recursive index/Merkle validation remain incomplete. |
| Rebuild      | partial | Recomputes document checksums and direct folder documents, but does not fully rebuild recursive `[[subfolders]]`/Merkle model. |
| Init         | done    | Creates `intake`, starter folders, type definitions, ontology, and runs rebuild. |
| Server       | partial | Uses `Arc<RwLock<LoadedVault>>`, read APIs, schemas, file/highlight APIs, validate/rebuild reloads; no watcher/write queue/mutations. |
| UI           | partial | Read-only browser is usable, supports custom icons, Documents/Files split, Markdown, JSON/code highlight, properties/edges/schema; no editing/repair/intake. |
| Agent        | partial | Crate skeleton exists, but no real provider/proposal/apply flow. |
| Tests        | done    | Current `cargo test`, clippy, and web check pass. |

## Implemented

- Canonical path-based IDs with mixed-case preservation in `crates/kataan-core/src/id.rs`.
- Metadata-only loaded vault in `crates/kataan-core/src/vault.rs`: `LoadedVault`, `DocumentRecord`, `read_markdown`, route-token map, ontology graph build.
- Markdown+TOML pair loading and standalone-file treatment in `crates/kataan-core/src/walk.rs`, `crates/kataan-core/src/vault.rs`, and `crates/kataan-core/src/validate.rs`.
- Registry-driven custom types in `crates/kataan-core/src/types.rs` and validation in `validate_type_registry` in `crates/kataan-core/src/validate.rs`.
- `code/` exception via `crates/kataan-core/src/constants.rs` and callers in vault loading, validation, rebuild, and server folder APIs.
- `intake` starter type replacing `raw` in `crates/kataan-core/src/init.rs`, `crates/kataan-core/templates/default-ontology.toml`, and test support.
- Ontology load/validation and inverse/symmetric graph behavior in `crates/kataan-core/src/ontology.rs` and `crates/kataan-core/src/graph.rs`.
- Validation diagnostics for TOML, checksums, type/folder mismatch, ontology edges, missing folders/indexes, actors/status, depth, and index drift in `crates/kataan-core/src/validate.rs`.
- Atomic writes in `crates/kataan-core/src/write.rs`; rebuild and init use `write::atomic_write_string`.
- Server state holds loaded vault in `crates/kataan-server/src/state.rs` and reloads after validate/rebuild in `crates/kataan-server/src/api.rs`.
- Read APIs in `crates/kataan-server/src/api.rs`: vault, folders, folder by canonical ID, document by ID, route resolve, schemas, raw file, highlighted file, validate, rebuild.
- File preview split: `/api/file` vs `/api/file/highlight` in `crates/kataan-server/src/api.rs`; Lumis theme-aware highlighting for JSON/TOML/Markdown/Rust/TS/JS/Bash/YAML/Python.
- Schema API in `crates/kataan-core/src/schema.rs` and `crates/kataan-server/src/api.rs`, including vault-aware allowed types and ontology predicates.
- Astro read-only UI in `apps/web/src/lib/dashboard.ts`, `apps/web/src/lib/api.ts`, and components/styles: folder tree, Documents/Files split, Markdown reader, highlighted file preview, properties/edges/internal/schema panels, custom Lucide icons.
- Tests cover custom type folders, metadata-only loading, recursive walk loading, route tokens, server folder/document/file/highlight endpoints, ontology graph behavior, validation, rebuild, init.

## Partially implemented

- `crates/kataan-core/src/rebuild.rs`: rebuild handles direct documents in each mapped type folder, but does not yet perform post-order recursive rebuild of nested folders or write `[[subfolders]]` entries.
- `crates/kataan-core/src/validate.rs`: validates nested folder index presence and many document invariants, but recursive Merkle/subfolder checksum validation is not fully aligned with the brief/plan.
- `crates/kataan-core/src/schema.rs`: schemas expose allowed types and predicates, but do not yet take `folder`/`id` query context to constrain a document schema to the current folder's allowed type.
- `apps/web/src/lib/dashboard.ts`: file previews work, but files/artifacts do not yet have URL locators or browser restore behavior.
- `crates/kataan-agent`: skeleton/provider boundary exists, but no complete provider-backed agent proposal workflow.
- Single-writer safety is only partially present: atomic write helpers exist, but server-side serialized mpsc write queue is not implemented.

## Missing

- Filesystem watcher with debounce, partial metadata patching, and full reload fallback.
- Global `FacetIndex` for labels + ancestors + type/status filtering.
- Full recursive `rebuild-indexes` with `[[subfolders]]`, post-order Merkle checksums, and root `updated_at` update.
- Case-insensitive canonical ID collision validation.
- Route-token collision validation.
- Edge mutation API and UI editing.
- Manual intake endpoint/UI.
- Repair UI for invalid TOML diagnostics and schema/LLM-assisted repair proposals.
- Serialized write command queue and apply-time base hash conflict detection.
- Real OpenAI/Anthropic provider flow, `/api/agent`, MCP surface, and proposal review/apply loop.
- File/artifact URL locators.

## Drift from spec

- Plan says `rebuild-indexes` should rebuild every document folder recursively, including `[[subfolders]]` and recursive folder checksums; current `crates/kataan-core/src/rebuild.rs` is still direct-folder oriented.
- Plan says server boot should validate/drift-detect and serve read-only with diagnostics on errors; current `AppState::new` loads `LoadedVault` directly and invalid regular document TOML is skipped by loader, while diagnostics are obtained through `/api/validate`.
- Brief/plan mention schema endpoint should support vault-aware constraints such as allowed type for current folder; current API is vault-aware globally, not context-aware per folder/id.
- Plan mentions single writer via mpsc queue; current write paths are command handlers calling core functions directly.
- Plan lists watcher behavior; no watcher implementation was found.
- Brief states highlighted HTML is sanitized; Lumis output is assumed safe, but there is no explicit sanitizer layer or test asserting escaping.

## Implemented but under-documented

- `/api/file/highlight` accepts an optional `theme` query parameter (`light` or `dark`) and selects different Lumis themes in `crates/kataan-server/src/api.rs`; brief/plan document the endpoint but not the query parameter or theme behavior.
- The web UI dispatches a `kataan:theme-change` event and re-fetches highlighted file HTML for the currently selected file in `apps/web/src/layouts/AppLayout.astro` and `apps/web/src/lib/dashboard.ts`; this behavior is not described in brief/plan.
- `/api/folders` includes an `icon` field derived from type definitions in `crates/kataan-server/src/api.rs`; docs describe Lucide icon metadata conceptually, but not the concrete API response field.
- Highlighting trims trailing newlines before Lumis rendering to avoid extra empty highlighted lines; this presentation detail is not documented and probably does not need spec-level documentation unless stable rendering is required.

## Test and command evidence

Commands run on 2026-04-30:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
bun --filter @kataan/web check
```

Summarized output:

- `cargo test`: passed; `kataan-core` 45 tests, `kataan-server` 10 tests, doc tests pass.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `bun --filter @kataan/web check`: passed; 14 files, 0 errors, 0 warnings, 0 hints.
- `git status --short`: no tracked/untracked changes printed after checks.

## Highest-risk invariant

Recursive folder index/Merkle correctness is the highest-risk invariant. The loader and UI already support nested folders and custom type folders, but `rebuild-indexes` and checksum validation are not yet fully recursive, so direct filesystem edits in nested document trees can drift from the stored index/checksum model without complete automatic repair.

## Recommended next actions

1. Implement full recursive `rebuild-indexes`: post-order walk, direct `[[documents]]`, `[[subfolders]]`, recursive folder checksums, `updated_at`, and `code/` exclusion.
2. Align validation with the recursive rebuild model, including subfolder checksum diagnostics and route-token/case-insensitive collision checks.
3. Add context-aware document schema constraints: `GET /api/schema/document?folder=...` or `?id=...`.
4. Add filesystem watcher and vault reload/diagnostic refresh path.
5. Add URL locators for files/artifacts or explicitly document that only document nodes are URL-addressable in v1.
6. Add manual intake endpoint/UI using the new `intake` type.
7. Add serialized write queue before implementing edit, repair, edge mutation, and agent apply flows.
