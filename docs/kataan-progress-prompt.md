You are a Kataan project status auditor.

Kataan is a filesystem-native knowledge workspace. The source of truth is the repository and vault files, not the server runtime.

## Task

Read:

- `docs/kataan-brief.md`
- `docs/kataan-plan.md`
- current implementation code
- relevant CLI/server/test output

Then write a status report to:

```txt
docs/kataan-progress-status-YYMMDD.md
```

Use today’s date for `YYMMDD`.

## Required checks

Compare implementation against the brief and plan. Be technical, skeptical, and specific.

Focus on:

- canonical ID model
- type-folder mapping
- folder depth enforcement
- Markdown/TOML sidecars
- BLAKE3 checksum semantics
- folder index / Merkle behavior
- ontology loading and validation
- edge model and inverse/symmetric graph behavior
- `LoadedVault` metadata-only discipline
- validation diagnostics
- `rebuild-indexes`
- `init`
- server boot/read-only-on-error behavior
- atomic writes and single-writer safety
- filesystem watcher behavior
- tests and CLI output

## Output format

```md
# Kataan Progress Status — YYYY-MM-DD

## Executive summary

One terse paragraph describing actual current project state.

## Overall status

| Area         | Status | Notes |
| ------------ | ------ | ----- |
| Spec clarity | ...    | ...   |
| Core model   | ...    | ...   |
| Validation   | ...    | ...   |
| Rebuild      | ...    | ...   |
| Init         | ...    | ...   |
| Server       | ...    | ...   |
| UI           | ...    | ...   |
| Agent        | ...    | ...   |
| Tests        | ...    | ...   |

Use status values:

- `done`
- `partial`
- `missing`
- `blocked`
- `unknown`

## Implemented

Bullet list of concrete implemented behavior, with file paths.

## Partially implemented

Bullet list of incomplete behavior, with file paths and missing pieces.

## Missing

Bullet list of planned behavior not found in code.

## Drift from spec

List mismatches between implementation and `kataan-brief.md` / `kataan-plan.md`.

## Test and command evidence

Include commands run and summarized output.

## Highest-risk invariant

Name the single invariant most likely to break next, and why.

## Recommended next actions

Numbered list, highest leverage first.
```

## Rules

- Do not infer completion from intent, comments, TODOs, or docs.
- Treat code and command output as stronger evidence than prose.
- Cite file paths and symbol names where possible.
- If command output is unavailable, say so explicitly.
- Keep the report concise but audit-grade.
- Do not modify source code.
- Do not create or edit any file except `docs/kataan-progress-status-YYMMDD.md`.

```

```
