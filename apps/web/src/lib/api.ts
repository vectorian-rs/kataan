const API_BASE = import.meta.env.PUBLIC_KATAAN_API_BASE ?? '';

import {
  array,
  boolean,
  type Infer,
  literals,
  number,
  object,
  optional,
  mapOf,
  record,
  type Shape,
  string,
} from './shape';

// Each response shape is declared once, below, and its TypeScript type is
// inferred from it. The shape checks the response at the boundary; the type is
// what the rest of the app sees. Two declarations of the same thing — a `type`
// and a validator — is the arrangement that drifts, so there is only one.
//
// `unknown` and `record` mark the places where the *vault* decides the shape,
// not kataan: a document's metadata keys, a JSON Schema, a `[nodes.*]`
// declaration. There is nothing to check those against here.

const vaultIndex = object({
  schema_version: string,
  name: string,
  created_at: optional(string),
  updated_at: optional(string),
  type_folders: record,
});
export type VaultIndex = Infer<typeof vaultIndex>;

/// Recursive: a field schema describes the interior of a table with more field
/// schemas. Declared as a lazy `Shape` because it refers to itself.
export type FieldSchema = {
  type: string;
  items?: string;
  to?: string[];
  description?: string;
  fields?: Record<string, FieldSchema>;
  required?: string[];
};
const fieldSchema: Shape<FieldSchema> = (value, path) =>
  object({
    type: string,
    items: optional(string),
    to: optional(array(string)),
    description: optional(string),
    // Recursive, and safe because this runs when the shape is *called*, by
    // which point `fieldSchema` is defined.
    fields: optional(mapOf(fieldSchema)),
    required: optional(array(string)),
  })(value, path) as FieldSchema;

const ontologyType = object({
  name: string,
  extends: optional(string),
  folders: array(string),
  required: array(string),
  fields: mapOf(fieldSchema),
  document_count: number,
  folder_index_count: number,
});
export type OntologyType = Infer<typeof ontologyType>;

const ontologyEdge = object({
  predicate: string,
  from: array(string),
  to: array(string),
  inverse: optional(string),
  symmetric: boolean,
  cardinality: optional(string),
  description: optional(string),
});
export type OntologyEdge = Infer<typeof ontologyEdge>;

const ontologyLink = object({ source: string, predicate: string, target: string });
export type OntologyLink = Infer<typeof ontologyLink>;

const ontologyResponse = object({
  types: array(ontologyType),
  edges: array(ontologyEdge),
  links: array(ontologyLink),
});
export type OntologyResponse = Infer<typeof ontologyResponse>;

const folderSummary = object({
  type: string,
  folder: string,
  name: optional(string),
  icon: optional(string),
  document_count: number,
});
export type FolderSummary = Infer<typeof folderSummary>;

const foldersResponse = object({ folders: array(folderSummary) });

const folderChild = object({ id: string, name: string, has_index: boolean });
export type FolderChild = Infer<typeof folderChild>;

const folderDocument = object({ id: string, slug: string, markdown: string, toml: string });
export type FolderDocument = Infer<typeof folderDocument>;

const folderFile = object({ name: string, path: string, extension: optional(string) });
export type FolderFile = Infer<typeof folderFile>;

const canonicalFolderResponse = object({
  id: string,
  metadata: optional(record),
  markdown: optional(string),
  folders: array(folderChild),
  documents: array(folderDocument),
  files: array(folderFile),
});
export type CanonicalFolderResponse = Infer<typeof canonicalFolderResponse>;

const documentResponse = object({
  id: string,
  type_folder: string,
  metadata: record,
  markdown: string,
  html: string,
});
export type DocumentResponse = Infer<typeof documentResponse>;

const fileResponse = object({
  path: string,
  name: string,
  extension: optional(string),
  kind: literals('html', 'json', 'text', 'image', 'pdf', 'unsupported'),
  content: string,
});
export type FileResponse = Infer<typeof fileResponse>;

const highlightResponse = object({
  path: string,
  name: string,
  extension: optional(string),
  language: string,
  html: string,
});
export type HighlightResponse = Infer<typeof highlightResponse>;

const resolvedDocument = object({
  id: string,
  folder: string,
  type_folder: string,
  is_folder_index: boolean,
});
export type ResolveResponse = Infer<typeof resolvedDocument>;

const diagnostic = object({
  severity: literals('error', 'warning', 'info'),
  code: string,
  message: string,
  path: optional(string),
});
export type Diagnostic = Infer<typeof diagnostic>;

const validateResponse = object({ ok: boolean, diagnostics: array(diagnostic) });
export type ValidateResponse = Infer<typeof validateResponse>;

const tomlSchemaResponse = object({
  kind: string,
  schema: record,
  constraints: object({
    allowed_status: array(string),
    allowed_actors: array(string),
    allowed_types: array(string),
    allowed_edge_predicates: array(string),
    notes: array(string),
  }),
  toml_template: string,
  /// The vault's `[nodes.<kind>]` declaration, when `kind` names a document
  /// type that has one. This is what makes the type different from every other
  /// document, and what the write boundary enforces.
  node_schema: optional(object({ required: array(string), fields: mapOf(fieldSchema) })),
});
export type TomlSchemaResponse = Infer<typeof tomlSchemaResponse>;

export type SearchQuery = {
  q?: string;
  kind?: 'document' | 'folder' | 'file';
  type?: string;
  status?: string;
  facet?: string;
  path_prefix?: string;
  limit?: number;
  offset?: number;
};

const searchResult = object({
  kind: literals('document', 'folder', 'file'),
  id: optional(string),
  path: string,
  title: optional(string),
  type: optional(string),
  status: optional(string),
  extension: optional(string),
  facets: array(string),
  snippet: optional(string),
  score: number,
});
export type SearchResult = Infer<typeof searchResult>;

const searchFacetCount = object({ facet: string, count: number });
export type SearchFacetCount = Infer<typeof searchFacetCount>;

const searchResponse = object({
  query: string,
  mode: literals('keyword'),
  results: array(searchResult),
  facets: array(searchFacetCount),
});
export type SearchResponse = Infer<typeof searchResponse>;

const searchStatus = object({
  index_path: string,
  exists: boolean,
  item_count: number,
  document_count: number,
  folder_count: number,
  file_count: number,
  last_indexed_at: optional(string),
});
export type SearchStatus = Infer<typeof searchStatus>;

const searchReindexResponse = object({
  ok: boolean,
  index_path: string,
  item_count: number,
  document_count: number,
  folder_count: number,
  file_count: number,
  indexed_at: string,
});
export type SearchReindexResponse = Infer<typeof searchReindexResponse>;

const okResponse = object({ ok: boolean });

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
  return getJson(`/api/resolve-path?path=${encodeURIComponent(path)}`, resolvedDocument);
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
