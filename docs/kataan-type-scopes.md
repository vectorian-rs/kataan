# Nested type scopes

Status: proposed, schema_version 0.2.0.

## The problem

A type has exactly one home, and that home must be a first-level directory.

`TypeDefinition.folder` is a single `String` (`types.rs`), and
`validate/folder.rs` compares it against `FolderWalk.root_folder`, which is the
top-level directory the walk started from:

```rust
match expected_type_folder {
    Some(expected_folder) if expected_folder != self.root_folder => TYPE_FOLDER_MISMATCH
```

So the ontology is isomorphic to the first path segment. In a real vault the
mapping is many-to-many. A deck can live at `presentations/the-garden-within/`,
at `companies/<org>/decks/<name>/`, or as one page inside a shared build
project at `companies/<org>/customers/<customer>/presentations/`. Only the
first can be typed `presentation`. The others are typed after their storage
tree, so a deck is typed `company`.

`default_type` does not help. `rebuild.rs` writes it when a folder index is
first created and nothing reads it during validation, so it documents intent
without enforcing it.

## The model

Three additions, each independent and each additive.

### 1. A type may claim several locations

`TypeDefinition.folder: String` becomes `folders: Vec<String>`, and each entry
is a path pattern rather than a directory name. `folder` stays accepted as a
deserialization alias holding a single-element list, so existing type
definitions keep parsing unchanged.

```toml
# type/presentation.toml
type = "type-definition"
name = "presentation"
folders = ["presentations", "companies/*/decks/*"]
markdown = "presentation.md"
```

Patterns are vault-root relative. `*` matches one path segment and never
crosses a `/`. There is no `**`: unbounded depth is what scopes are for.

### 2. A folder may declare types for its own subtree

A folder's `index.toml` may carry a `[type_folders]` table. Keys are type
names, values are patterns relative to **the declaring folder**. The
declaration is additive for that folder and everything under it, and invisible
outside it.

```toml
# companies/snappy/decks/index.toml
type = "company"
name = "Decks"

[type_folders]
deck = "."
```

```toml
# companies/snappy/customers/index.toml
[type_folders]
customer = "."
presentation = "*/presentations"
```

`"."` means the declaring folder and its descendants. This is the mechanism
that lets the ontology grow at depth without the root config accumulating an
entry for every tree in the vault.

### 3. A type may extend another

```toml
# type/customer.toml
name = "customer"
extends = "company"
folders = ["companies/*/customers/*"]
```

`extends` forms the subtype relation. Everywhere a type is matched against a
set of permitted types, the match walks the `extends` chain. An edge declared
`from = ["company"]` therefore accepts a `customer` source without
`ontology.toml` being touched. Cycles are a validation error, and the chain is
resolved once at registry load.

## Resolution

For a document at vault-relative path `P` declaring type `T`:

1. Build the **scope chain**: the vault root, then every ancestor folder of `P`
   whose `index.toml` declares `[type_folders]`, ordered root first. The
   validator already recurses parent before child, so the chain is accumulated
   during the walk rather than rediscovered per document.
2. Collect **claims** for `T`: from the root scope, `kataan.toml
   [type_folders]` plus the type registry's `folders`; from each folder scope,
   its `[type_folders]` entry for `T`, resolved against the declaring folder.
3. `T` is legal at `P` if any claim matches an ancestor directory of `P`.
   Any match permits.
4. The **default** type for a folder is taken from the nearest scope that
   claims it. Nearest wins, so a subtree can narrow `company` to `customer`
   without the outer declaration interfering.

Matching is on directories, not on the document file, so a claim of
`companies/*/decks/*` covers `companies/snappy/decks/hpc-graviton/index.toml`
and every document inside it.

Rule 3 is deliberately permissive and rule 4 deliberately specific. Legality is
a union because a deck genuinely does belong in more than one place; defaulting
is nearest-wins because there has to be exactly one answer when creating a
document.

## Depth

`max_folder_depth` is currently measured from the vault root by
`CanonicalId::depth_after_type_folder`. With nested scopes that budget is spent
on reaching the scope, not on the structure inside it, so depth is measured
from the **nearest declaring scope** instead. A vault with no folder-level
declarations measures exactly as it does today.

## Diagnostics

- `TYPE_FOLDER_MISMATCH` keeps its code. The message gains the claims that were
  considered: ``document type `deck` is not claimed at `companies/x/y`; claims:
  `companies/*/decks/*` (root), `.` (companies/snappy/decks)``.
- `INVALID_TYPE` is unchanged.
- New `TYPE_EXTENDS_CYCLE` for a cycle in the `extends` chain.
- New `TYPE_SCOPE_UNKNOWN_TYPE` when a folder `[type_folders]` names a type
  with no definition in `type/`.
- New `TYPE_SCOPE_ESCAPES` when a folder declaration resolves outside its own
  subtree. The same threat as `is_safe_type_folder`: vaults are shared as git
  repositories, so these values are untrusted, and a declaration must never
  reach above the folder that wrote it.

## Back-compat

The proof obligation is that a vault which validates clean today still
validates clean with no edits.

- `folder = "x"` deserializes to `folders = ["x"]`.
- A vault with no folder-level `[type_folders]` has a scope chain of length one,
  the root, so resolution reduces to the current equality check.
- `extends` absent means the ancestry walk terminates immediately, so type
  matching stays exact.
- `default_type` keeps its current meaning and write path. It is now also read
  as the nearest-scope default when no `[type_folders]` claim applies.

The snuffbox vault (723 sidecars, 682 typed nodes, `ok: true` with zero
diagnostics) is the regression fixture.

## Migration

Staged, and each stage validates clean before the next.

1. Land the resolver with the above back-compat, migrating nothing.
2. Decks pilot: add `deck`, widen `presentation`, re-type
   `companies/*/decks/*` off `company`, give each customer presentations
   project a node, one node per deck.
3. Later, and separately: split `company` into `customer`, `partner`, `sow`,
   `engagement` via `extends`, and fold the ad-hoc `kind` field (137 uses, 13
   values) into real types on `organizations/`.
