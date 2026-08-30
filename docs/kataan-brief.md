# Kataan

Kataan is a simple, filesystem-native knowledge workspace where humans and agents collaborate to turn intake information into organized markdown knowledge.

It is inspired by AI-compiled knowledge bases, but it is not AI-only. Humans can create, edit, and organize content directly. Agents help with intake, classification, restructuring, summarization, and maintenance.

Documents are connected with edges defined in the relationship ontology (`ontology.toml`).

## Where the detail lives

This brief states the idea. Two companion documents carry the specifics:

- [`kataan-format.md`](kataan-format.md) — the on-disk contract: vault
  structure, the file model, folder and type mapping, checksums, identity,
  the ontology, metadata fields, labels, types, and intake provenance.
- [`kataan-operations.md`](kataan-operations.md) — agent and human access,
  concurrency and writes, boot and API modes, the MCP surface, TOML schema
  repair guidance, and diagnostics.
