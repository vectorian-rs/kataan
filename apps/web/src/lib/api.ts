const API_BASE = import.meta.env.PUBLIC_KATAAN_API_BASE ?? '';

import type { Shape } from './shape';

// Every response shape lives in `./api-shapes.generated.ts`, emitted from the
// Rust structs by `crates/kataan-server/src/shapes.rs`. They are not types
// sitting beside a validator: each shape validates the response *and* is the
// source of the TypeScript type, so there is one declaration and Rust owns it.
//
// This file owns the other half — which URL each response comes from, and what
// a request looks like. Those are not in the structs, so they are written here.
export * from './api-shapes.generated';

import type { SearchResultKind } from './api-shapes.generated';
import {
  canonicalFolderResponse,
  documentResponse,
  fileResponse,
  foldersResponse,
  highlightResponse,
  okResponse,
  ontologyResponse,
  resolveResponse,
  searchReindexResponse,
  searchResponse,
  searchStatus,
  tomlSchemaResponse,
  validateResponse,
  vaultIndex,
} from './api-shapes.generated';

/// What a search request may ask for.
///
/// A request, not a response, so it is written here rather than generated:
/// `SearchQuery` on the Rust side is a `Deserialize` target whose fields are
/// all optional strings by the time they reach it as query parameters.
export type SearchQuery = {
  q?: string;
  kind?: SearchResultKind;
  type?: string;
  status?: string;
  facet?: string;
  path_prefix?: string;
  limit?: number;
  offset?: number;
};

export async function getVault() {
  return getJson('/api/vault', vaultIndex);
}

export async function getFolders() {
  return getJson('/api/folders', foldersResponse);
}

/// A canonical id as a URL path.
///
/// Encoded per segment, so the slashes that separate id segments stay slashes
/// and everything else is escaped. Ids are `[a-z0-9-]` by grammar, so nothing
/// here actually needs escaping today — it is written this way so the rule
/// holds if the grammar ever widens, and so reads and writes address a document
/// identically.
function idPath(id: string) {
  return id.split('/').map(encodeURIComponent).join('/');
}

export async function getFolder(id: string) {
  return getJson(`/api/folders/${idPath(id)}`, canonicalFolderResponse);
}

/// Resolve a vault path (or a canonical id, which is the extensionless form)
/// to the document it names.
export async function resolvePath(path: string) {
  return getJson(`/api/resolve-path?path=${encodeURIComponent(path)}`, resolveResponse);
}

export async function getOntology() {
  return getJson('/api/ontology', ontologyResponse);
}

export async function getDocument(id: string, theme?: string) {
  // Fenced code blocks are highlighted server-side, so the theme has to travel
  // with the request; without it every block renders dark in light mode.
  const query = theme ? `?theme=${encodeURIComponent(theme)}` : '';
  return getJson(`/api/documents/${idPath(id)}${query}`, documentResponse);
}

/// Save a document's Markdown body.
///
/// `expectedUpdatedAt` is the `updated_at` the editor loaded. The server
/// refuses with 409 if the document changed since, so a save cannot silently
/// discard an edit made in a text editor or by an agent while this tab sat
/// open.
/// Everything one Save writes. Omitted keys are left alone; a `null` inside
/// `fields` removes that key.
export type DocumentEdit = {
  body?: string;
  status?: string | null;
  aliases?: string[];
  labels?: string[];
  occurred_at?: string | null;
  fields?: Record<string, unknown>;
};

export async function updateDocument(id: string, edit: DocumentEdit, expectedUpdatedAt?: string) {
  const response = await fetch(`${API_BASE}/api/documents/${idPath(id)}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ ...edit, expected_updated_at: expectedUpdatedAt }),
  });
  if (response.status === 409) {
    throw new Error(
      'This document changed on disk since you opened it. Reload to see the current version — your text is still here until you do.',
    );
  }
  if (!response.ok) {
    throw new Error(`Save failed: ${response.status} ${await response.text()}`);
  }
  return okResponse(await response.json(), 'PATCH /api/documents');
}

export async function getFile(path: string) {
  return getJson(`/api/file?path=${encodeURIComponent(path)}`, fileResponse);
}

export async function getHighlightedFile(path: string, theme?: string) {
  const params = new URLSearchParams({ path });
  if (theme) params.set('theme', theme);
  return getJson(`/api/file/highlight?${params.toString()}`, highlightResponse);
}

export function getRawFileUrl(path: string) {
  return `${API_BASE}/api/file/raw?path=${encodeURIComponent(path)}`;
}

export async function getSchema(kind: string) {
  return getJson(`/api/schema/${encodeURIComponent(kind)}`, tomlSchemaResponse);
}

export async function validateVault() {
  return postJson('/api/validate', validateResponse);
}

export async function rebuildIndexes() {
  return postJson('/api/rebuild-indexes', okResponse);
}

export async function searchVault(query: SearchQuery) {
  const params = new URLSearchParams();
  appendQueryParam(params, 'q', query.q);
  appendQueryParam(params, 'kind', query.kind);
  appendQueryParam(params, 'type', query.type);
  appendQueryParam(params, 'status', query.status);
  appendQueryParam(params, 'facet', query.facet);
  appendQueryParam(params, 'path_prefix', query.path_prefix);
  appendQueryParam(params, 'limit', query.limit);
  appendQueryParam(params, 'offset', query.offset);
  return getJson(`/api/search?${params.toString()}`, searchResponse);
}

export async function getSearchStatus() {
  return getJson('/api/search/status', searchStatus);
}

export async function reindexSearch() {
  return postJson('/api/search/reindex', searchReindexResponse);
}

function appendQueryParam(
  params: URLSearchParams,
  key: string,
  value: string | number | undefined,
) {
  if (value === undefined || value === '') return;
  params.set(key, String(value));
}

async function getJson<T>(path: string, shape: Shape<T>): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`);
  return parseJson(response, shape, label(path));
}

/// The endpoint, without its query string, to name in a validation failure.
/// The query is noise once you know which route answered.
function label(path: string) {
  return path.split('?')[0];
}

async function postJson<T>(path: string, shape: Shape<T>): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, { method: 'POST' });
  return parseJson(response, shape, label(path));
}

async function parseJson<T>(response: Response, shape: Shape<T>, path: string): Promise<T> {
  if (response.ok) {
    // Checked, not asserted. `as Promise<T>` would let a changed response shape
    // travel as far as the DOM before anything noticed, and the symptom would
    // be an `undefined` nowhere near the cause.
    return shape(await response.json(), path);
  }

  const errorMessage = await readErrorMessage(response);
  throw new Error(`API request failed: ${response.status} ${response.statusText}: ${errorMessage}`);
}

async function readErrorMessage(response: Response) {
  try {
    const body = (await response.json()) as { error?: string };
    return body.error ?? 'Unknown API error';
  } catch {
    return 'Unknown API error';
  }
}
