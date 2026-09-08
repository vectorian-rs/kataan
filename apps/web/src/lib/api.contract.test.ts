//! Does the server still send what this client expects?
//!
//! The response shapes in `api.ts` are hand-written mirrors of Rust structs,
//! and nothing makes Rust and TypeScript agree — see #45. Since 104bcd0 those
//! shapes are enforced at run time, so a disagreement is no longer a silently
//! wrong render; it is a refused response. That is better, but it still means a
//! user finds out.
//!
//! This runs the real client against a real server over a real vault, so a
//! shape that has drifted from its struct fails here, on the commit that
//! caused it, rather than in someone's browser. It calls the exported functions
//! rather than fetching URLs directly, so the URL each one builds is under test
//! too.

import { afterAll, beforeAll, expect, test } from 'bun:test';
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const repoRoot = resolve(import.meta.dir, '../../../..');

/// Debug first: `mise run check` runs `cargo test` before this, which builds
/// the debug binaries.
function binary(name: string) {
  for (const profile of ['debug', 'release']) {
    const path = join(repoRoot, 'target', profile, name);
    if (existsSync(path)) return path;
  }
  throw new Error(
    `${name} is not built. This test drives the real server, so build it first:\n` +
      `  cargo build -p kataan-server -p kataan-cli`,
  );
}

async function run(command: string[]) {
  const process = Bun.spawn({ cmd: command, stdout: 'pipe', stderr: 'pipe' });
  const code = await process.exited;
  if (code !== 0) {
    throw new Error(
      `${command.join(' ')} exited ${code}: ${await new Response(process.stderr).text()}`,
    );
  }
}

/// A port the OS says is free. Racy in principle; the window is microseconds
/// and the alternative is a hardcoded port that collides with a real server —
/// this repo has one running on 3001.
function freePort() {
  const probe = Bun.serve({ port: 0, fetch: () => new Response('') });
  const { port } = probe;
  probe.stop(true);
  return port;
}

let vault: string;
let server: ReturnType<typeof Bun.spawn>;
let api: typeof import('./api');

beforeAll(async () => {
  vault = mkdtempSync(join(tmpdir(), 'kataan-contract-'));
  await run([binary('kataan-cli'), 'init', vault, '--name', 'Contract']);

  // A document pair and a plain file, so the document and file endpoints have
  // something of each to answer with.
  writeFileSync(join(vault, 'notes/sample.md'), '# Sample\n\nBody text.\n');
  writeFileSync(join(vault, 'notes/sample.toml'), 'type = "note"\nmarkdown = "sample.md"\n');
  mkdirSync(join(vault, 'code'), { recursive: true });
  // A recognised language: `/api/file/highlight` refuses a file it has no
  // lexer for, so a `.txt` would test the error path rather than the shape.
  writeFileSync(join(vault, 'code/hello.rs'), 'fn main() {\n    println!("hi");\n}\n');
  await run([binary('kataan-cli'), 'rebuild-indexes', vault]);

  const port = freePort();
  const base = `http://127.0.0.1:${port}`;
  server = Bun.spawn({
    cmd: [binary('kataan-server'), '--vault', vault, '--bind', `127.0.0.1:${port}`],
    stdout: 'pipe',
    stderr: 'pipe',
  });

  const deadline = Date.now() + 20_000;
  for (;;) {
    try {
      if ((await fetch(`${base}/api/health`)).ok) break;
    } catch {
      // not listening yet
    }
    if (Date.now() > deadline) throw new Error('server never became reachable');
    await Bun.sleep(150);
  }

  // Set before the import: `API_BASE` is read once, when the module loads.
  process.env.PUBLIC_KATAAN_API_BASE = base;
  api = await import('./api');
});

afterAll(() => {
  server?.kill();
  if (vault) rmSync(vault, { recursive: true, force: true });
});

// Every call below validates its response against the shape the app uses. A
// mismatch throws with the field that disagreed, so "it resolved" is the
// assertion — the extra checks are there to prove the call reached real data
// rather than an empty success.

test('vault index', async () => {
  const index = await api.getVault();
  expect(index.name).toBe('Contract');
  expect(Object.keys(index.type_folders).length).toBeGreaterThan(0);
});

test('folder list', async () => {
  const { folders } = await api.getFolders();
  expect(folders.map((folder) => folder.folder)).toContain('notes');
});

test('folder detail', async () => {
  const folder = await api.getFolder('notes');
  expect(folder.id).toBe('notes');
  expect(folder.documents.map((document) => document.id)).toContain('notes/sample');
});

test('document', async () => {
  const document = await api.getDocument('notes/sample');
  expect(document.id).toBe('notes/sample');
  expect(document.html).toContain('<h1');
  expect(document.metadata.type).toBe('note');
});

test('document with a theme, since the query string is part of the call', async () => {
  const document = await api.getDocument('notes/sample', 'light');
  expect(document.markdown).toContain('Body text.');
});

test('resolve a path to an id', async () => {
  const resolved = await api.resolvePath('notes/sample.md');
  expect(resolved.id).toBe('notes/sample');
  expect(resolved.is_folder_index).toBe(false);
});

test('ontology', async () => {
  const ontology = await api.getOntology();
  expect(ontology.types.length).toBeGreaterThan(0);
  expect(ontology.edges.length).toBeGreaterThan(0);
});

test('file, and the same file highlighted', async () => {
  const file = await api.getFile('code/hello.rs');
  expect(file.kind).toBe('text');
  expect(file.content).toContain('println!');

  const highlighted = await api.getHighlightedFile('code/hello.rs', 'light');
  expect(highlighted.language).toBe('rust');
  expect(highlighted.html).toContain('<span');
});

test('schema for a kataan kind and for a vault type', async () => {
  const document = await api.getSchema('document');
  expect(document.kind).toBe('document');
  expect(document.constraints.allowed_status.length).toBeGreaterThan(0);

  // A vault type takes the other branch, the one that can carry `node_schema`.
  expect((await api.getSchema('note')).kind).toBe('note');
});

test('validate and rebuild', async () => {
  expect((await api.validateVault()).ok).toBe(true);
  expect((await api.rebuildIndexes()).ok).toBe(true);
});

test('search: reindex, status, query', async () => {
  const reindexed = await api.reindexSearch();
  expect(reindexed.ok).toBe(true);

  const status = await api.getSearchStatus();
  expect(status.exists).toBe(true);
  expect(status.item_count).toBeGreaterThan(0);

  const results = await api.searchVault({ q: 'sample', limit: 10 });
  expect(results.mode).toBe('keyword');
  expect(results.results.map((result) => result.id)).toContain('notes/sample');
});

test('a write, and the read that follows it', async () => {
  const before = await api.getDocument('notes/sample');
  const written = await api.updateDocument(
    'notes/sample',
    { body: '# Sample\n\nRewritten.\n', labels: ['contract'] },
    typeof before.metadata.updated_at === 'string' ? before.metadata.updated_at : '',
  );
  expect(written.ok).toBe(true);

  const after = await api.getDocument('notes/sample');
  expect(after.markdown).toContain('Rewritten.');
  expect(after.metadata.labels).toEqual(['contract']);
});
