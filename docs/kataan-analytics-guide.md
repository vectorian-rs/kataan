# Building analytics on a kataan vault

For an agent or service computing metrics, rollups, or dashboards over a vault.
Covers what the API and MCP surfaces gained recently, the contracts you can rely
on, and the places that will bite you.

Everything here is available identically on **HTTP** (`kataan-server`), **MCP**
(`kataan-mcp`), and mostly on the **CLI** (`kataan-cli`) — one implementation in
`kataan_core::query`, three thin adapters. Pick by deployment: CLI if you run
inside the vault repo, HTTP if you have a server, MCP if you are an LLM agent.

---

## The short version

| You want | Use |
| :-- | :-- |
| Every document of a type | `documents(type)` |
| A batch of known ids | `documents(ids)` |
| One document's relationships | `neighbors(id)` |
| The whole graph, once | `subgraph()` / `kataan graph export` |
| An id from a filesystem path | `resolve_path(path)` |
| Full-text ranked hits | `search(q)` |

Before these existed, rebuilding a graph artifact cost one round trip per
document. It is now one call.

---

## Bulk read: `documents()`

```
documents(ids?, type?, status?, labels?, path_prefix?, linked_to?,
          predicate?, direction?, include?, limit?, offset?)
```

HTTP: `GET /api/documents?type=organization&limit=1000`
MCP: the `documents` tool
CLI: `kataan documents <vault> --type organization --limit 1000`

Returns `{ documents: [...], missing: [...], total }`.

### The paging contract — read this one

**Omitting `limit`** means you have not thought about page size. If more
documents match than the default limit, the call **errors** rather than handing
back a truncated list that looks complete. This is deliberate: a partial answer
silently mistaken for a whole one is the failure mode that corrupts a rollup.

**Passing an explicit `limit`** means you have opted into paging. You get at
most `limit` documents, and `total` reports the full match count so you know how
far to go:

```
offset=0   limit=100 -> documents[0..100],   total=250
offset=100 limit=100 -> documents[100..200], total=250
offset=200 limit=100 -> documents[200..250], total=250
offset=300 limit=100 -> [],                  total=250
```

Reading past the end is empty, not an error. `total` never depends on paging.
The hard ceiling on `limit` is 1000; above that is an error.

> Fixed on 2026-08-30. Before that, paging forward was impossible — only the
> final page was reachable. If you pinned an older build, re-check your
> pagination.

### `include`

| Value | Returns | Cost |
| :-- | :-- | :-- |
| `metadata` (default) | the summary: id, type, title, status, labels, `is_folder_index` | free |
| `full` | the summary plus each document's complete metadata — declared `[nodes.*]` fields, `occurred_at`, `edges`, and any key kataan does not model | **free** |
| `markdown` | the summary plus the body | one filesystem read per document |

`full` is free because `LoadedVault` already holds every document's metadata in
memory; only bodies are left on disk. If you are projecting scalars — a
`website`, a `period`, an `occurred_at` — ask for `full` rather than fetching
each document individually.

A document whose body cannot be read is reported in `missing`, not silently
dropped — so `documents.len() + missing.len()` always reconciles against what
you asked for.

### `ids` batch fetch

Order is preserved. Ids that do not resolve come back in `missing` rather than
failing the whole batch, so one stale id in a list of 200 does not cost you the
other 199.

### `linked_to`

Filters to documents with an edge to a given id, optionally narrowed by
`predicate` and `direction`. It reuses the same traversal as `neighbors`, so the
two agree by construction:

```
documents(linked_to: "organizations/bull", predicate: "works_at", direction: "in")
```

---

## Graph: `neighbors()` and `subgraph()`

### `neighbors(id, predicate?, direction?)`

```json
{
  "id": "organizations/bull",
  "out": { "owns": [ {"id": "...", "type": "...", "title": "...", "status": "..."} ] },
  "in":  { "works_at": [ ... ] }
}
```

**This is the only way to read incoming edges.** `get_document` returns the raw
`edges` table, which is outgoing-only. `works_at` is declared on each *person*,
so "who works at this organization" is structurally unanswerable from the
organization's own document — the inverse exists only in the graph. Incoming
edges are keyed by the ontology's declared inverse predicate (`works_at`
outgoing appears as whatever `inverse` names).

Nodes come back **hydrated** with type, title, status, and labels, so you can
aggregate without a second fetch per neighbour.

### `subgraph(types?, predicates?)`

```json
{ "nodes": [ {"id","type","title","status","labels","is_folder_index"} ],
  "links": [ {"source","predicate","target"} ] }
```

Guarantees worth relying on:

- **Each relationship appears exactly once**, in the direction it was authored.
  Derived inverses are not emitted as extra links, and a symmetric edge
  declared from both endpoints still yields one link. If you are summing edge
  weights, you are not double counting.
- **Internally consistent under filters.** A link is kept only when both
  endpoints survive the `types` filter, so no link ever dangles at a node that
  is not in `nodes`.
- **Deterministic.** Byte-identical across runs, so `kataan graph export` output
  diffs cleanly and can be committed as a build artifact.

### Two things the edge model will not do for you

**An edge is identified by `(source, predicate, target)`, so a relationship
cannot repeat.** Someone who worked at an employer, left, and returned is one
`worked_at` edge, not two. `add_edge` refuses the duplicate on write and the
graph stores targets in a set, so the second occurrence is not lost late — it is
never representable. If you need the two spells distinguished, reify the
relationship as a document with its own interval and point at it.

**`reference` fields are not edges.** `VaultGraph` is built purely from
`metadata.edges`, so a field declared `{ type = "reference" }` is validated (the
target must exist, and match `to` if given) but is invisible to `neighbors`,
`subgraph`, and `linked_to`. Use `[edges]` for anything you intend to traverse.

`subgraph` has no `limit`. A full export of a ~700-document vault is fine over
HTTP and CLI; over MCP it is a large response, so filter by `types` and
`predicates`, or prefer `neighbors` when you only need one document.

### Subtypes change what a type filter returns

A type may declare `extends`, and type matching walks that chain. This is not
cosmetic — it changes counts:

```
documents(type: "company")   ->  companies *and* customers, partners, ...
subgraph(types: ["company"]) ->  the same widening, for nodes and their links
```

The rule is "is a", applied wherever a type meets a set of allowed types: query
filters, `subgraph` type filters, and an edge's declared `from`/`to`. With no
`extends` in the vault it degrades to an equality test, so nothing changes until
someone introduces a subtype — at which point a rollup written against
`type = "company"` silently starts including the subtypes.

That is the intended semantics: a customer *is* a company, and a total that
excluded it would be wrong. But if you want the strict type, filter the returned
`type` field yourself; there is no "exact match" flag. Read `type/*.toml` for the
`extends` chains in play before treating a per-type count as a partition.

### `is_folder_index` — do not skip this

Some nodes are folder index documents rather than leaf entities. Most are
containers you want to exclude from entity counts (a `people` folder is not a
person), but **some are genuine entities that own edges**. In one real vault, 2
of 135 folder indexes were real linked entities.

Kataan cannot tell them apart, so it reports the flag and leaves the choice to
you. Filtering all of them out will silently drop real nodes and their edges;
keeping all of them inflates entity counts. Decide per vault, and check whether
the node participates in any link.

---

## Time

Three optional fields on every document:

| Field | Meaning | Who sets it |
| :-- | :-- | :-- |
| `occurred_at` | when the thing described happened (valid time) | the author |
| `created_at` | when the record was written (transaction time) | the mutation layer |
| `updated_at` | when the record last changed | the mutation layer |

**Dates are RFC 3339, and only RFC 3339.** A value is one of:

```
2026-08-29                 a calendar day  (RFC 3339 full-date)
2026-08-29T12:00:00Z       a moment        (RFC 3339 date-time)
```

Reduced precision — `2026`, `2026-08` — is ISO 8601 but not RFC 3339 and is
rejected, so you never have to decide how to bucket a value whose month is
unknown. A year on its own is not a date; it appears as a number in its own
field.

Values are validated on write and by `kataan validate`: bare Unix epochs,
zoneless datetimes, and impossible dates like `2026-02-30` are rejected.

Dates are always **quoted strings**, never TOML's native date type. An unquoted
`signed_on = 2024-01-02` is a distinct TOML value that does not survive
serialization intact — `toml` renders it as a table keyed
`$__toml_private_datetime`, so a consumer round-tripping metadata sees a table
where the author wrote a date. `validate` reports it as `native-toml-datetime`,
including inside nested tables and arrays. So every date you read is a string,
in one of the two forms above.

**Not yet available:** `after` / `before` / `order` filters on `documents()`.
Sort client-side for now. A root time index sorted on `(occurred_at, type, id)`
is planned.

`updated_at` is stamped on document updates *and* on edge writes, and a no-op
update does not move it — so it is a usable "what changed recently" signal.

---

## Custom fields are visible now

Kataan preserves and returns top-level sidecar keys it does not model. Fields
like `website`, `linkedin`, `company_id`, `kind`, `source_url` come back in
document metadata, flattened alongside kataan's own keys.

This is new. Previously they were invisible to every consumer *and* destroyed on
write, so if you have historical extracts, they are missing these.

A vault can constrain them with `[nodes.*]` schemas in `ontology.toml`, which
gives you a machine-readable description of what a type carries — useful for
deciding what is safe to aggregate.

The accurate rule about nesting: kataan validates a **table it has a type for**,
which today means `interval`. So this is checked, including that `to` is not
before `from`:

```toml
[nodes.employment.fields]
period = { type = "interval" }
```

What it cannot do is reach inside a table it has no type for. Declaring
`rate_card = { type = "table" }` requires the table to exist and be a table, but
nothing constrains `rate_card.effective_date`. Model dated things as `interval`
and they are validated; a lot of vault dates currently sit inside untyped tables
where they are not.

**`NodeSchema` has no cross-field validation.** `required` is an unconditional
list and no rule spans two fields, so invariants like "an open interval must
carry `confirmed_at`" are yours to enforce.

---

## Paths to ids

`resolve_path(path)` maps a filesystem path to a canonical id. Accepts either
file of a pair (`notes/x.md`, `notes/x.toml`), a folder's `index`, the
extensionless form, and absolute paths inside the vault root — so a joined
`REPO + relative` path works directly.

Returns not-found for a well-formed path to a nonexistent document, so you never
get a dangling id. A path that is not a kataan document (a `_template.md` with
no sidecar) correctly does not resolve: this is *document* resolution, not file
resolution.

---

## Gotchas

**MCP and HTTP do not return identical shapes for `resolve`.** HTTP returns five
fields; MCP returns `{id}` for `resolve` and `{id, is_folder_index}` for
`resolve_path`. Do not assume parity there. The bulk and graph reads *are*
identical across surfaces — verified byte-for-byte.

**MCP reloads the whole vault per call.** Every tool call re-reads and re-parses
every document. Fine for interactive use; if you are issuing hundreds of calls,
use HTTP (which holds the vault in memory) or the CLI.

**`search` is ranked full-text, not a filter.** It requires query text and
returns BM25 hits. For "every document of type X", use `documents()` — that is
what it is for. Query text that tokenises to nothing (`C++`, `&&`, an emoji)
matches nothing rather than returning the whole vault.

**Facet counts in search results reflect the returned page**, not the full match
set, so they change as you page. Do not build a facet sidebar on them yet.

**Writes are MCP-only.** The HTTP API is read-only apart from `validate` and
`rebuild-indexes`. Edges can be added, removed, and replaced wholesale for one
predicate (`add_edge`, `remove_edge`, `replace_edges_for_predicate`); only
`add_edge` and the replacement's incoming targets are ontology-validated, since
an edge worth removing is often one the ontology now forbids.

---

## Reproducible extracts

For a pipeline that needs a stable snapshot, run inside the vault repo:

```sh
kataan graph export <vault> --type person,organization \
    --predicate works_at,owns,invested_in > graph.json
kataan documents <vault> --type organization --limit 1000 > organizations.json
```

Both write JSON to stdout, both are deterministic, and both are safe to pipe
into `jq` or `head`. No server required, and the output can be committed so a
downstream diff shows exactly what changed between runs.
