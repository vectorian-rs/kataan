// Generated from the Rust response types. Do not edit.
//
// Every shape below mirrors a struct some handler returns as `Json<_>`. It is
// emitted by `crates/kataan-server/src/shapes.rs`, which is a test: change a
// response struct without regenerating and `cargo test` fails, rather than a
// user's browser.
//
//     KATAAN_UPDATE_SHAPES=1 cargo test -p kataan-server shapes
//
// A shape is both the runtime validator and the source of its TypeScript type,
// so there is nothing here for a human to keep in step.

import {
  array,
  boolean,
  json,
  literals,
  mapOf,
  number,
  object,
  openObject,
  optional,
  string,
  type Infer,
  type Shape,
} from './shape';

export const folderDocument = object({
  id: string,
  markdown: string,
  slug: string,
  toml: string,
});
export type FolderDocument = Infer<typeof folderDocument>;

export const folderFile = object({
  extension: optional(string),
  name: string,
  path: string,
});
export type FolderFile = Infer<typeof folderFile>;

export const folderChild = object({
  has_index: boolean,
  id: string,
  name: string,
});
export type FolderChild = Infer<typeof folderChild>;

export const documentMetadata = openObject({
  aliases: array(string),
  created_at: optional(string),
  created_by: optional(string),
  edges: mapOf(array(string)),
  labels: array(string),
  last_updated_by: optional(string),
  markdown: string,
  markdown_checksum: optional(string),
  occurred_at: optional(string),
  status: optional(string),
  type: string,
  updated_at: optional(string),
});
export type DocumentMetadata = Infer<typeof documentMetadata>;

export const canonicalFolderResponse = object({
  documents: array(folderDocument),
  files: array(folderFile),
  folders: array(folderChild),
  id: string,
  markdown: optional(string),
  metadata: optional(documentMetadata),
});
export type CanonicalFolderResponse = Infer<typeof canonicalFolderResponse>;

export const documentResponse = object({
  html: string,
  id: string,
  markdown: string,
  metadata: documentMetadata,
  type_folder: string,
});
export type DocumentResponse = Infer<typeof documentResponse>;

export const fileKind = literals('html', 'json', 'text', 'image', 'pdf', 'unsupported');
export type FileKind = Infer<typeof fileKind>;

export const fileResponse = object({
  content: string,
  extension: optional(string),
  kind: fileKind,
  name: string,
  path: string,
});
export type FileResponse = Infer<typeof fileResponse>;

export const folderSummary = object({
  document_count: number,
  folder: string,
  icon: optional(string),
  name: optional(string),
  type: string,
});
export type FolderSummary = Infer<typeof folderSummary>;

export const foldersResponse = object({
  folders: array(folderSummary),
});
export type FoldersResponse = Infer<typeof foldersResponse>;

export const highlightResponse = object({
  extension: optional(string),
  html: string,
  language: string,
  name: string,
  path: string,
});
export type HighlightResponse = Infer<typeof highlightResponse>;

export const okResponse = object({
  ok: boolean,
});
export type OkResponse = Infer<typeof okResponse>;

export const ontologyEdge = object({
  cardinality: optional(string),
  description: optional(string),
  from: array(string),
  inverse: optional(string),
  predicate: string,
  symmetric: boolean,
  to: array(string),
});
export type OntologyEdge = Infer<typeof ontologyEdge>;

export const ontologyLink = object({
  predicate: string,
  source: string,
  target: string,
});
export type OntologyLink = Infer<typeof ontologyLink>;

export const fieldType = literals(
  'string',
  'integer',
  'number',
  'boolean',
  'array',
  'table',
  'date',
  'instant',
  'interval',
  'reference',
);
export type FieldType = Infer<typeof fieldType>;

export type FieldSchema = {
  description?: string;
  fields: Record<string, FieldSchema>;
  items?: FieldType;
  required: string[];
  to: string[];
  type: FieldType;
};
export const fieldSchema: Shape<FieldSchema> = (value, path) =>
  object({
    description: optional(string),
    fields: mapOf(fieldSchema),
    items: optional(fieldType),
    required: array(string),
    to: array(string),
    type: fieldType,
  })(value, path) as FieldSchema;

export const ontologyType = object({
  document_count: number,
  extends: optional(string),
  fields: mapOf(fieldSchema),
  folder_index_count: number,
  folders: array(string),
  name: string,
  required: array(string),
});
export type OntologyType = Infer<typeof ontologyType>;

export const ontologyResponse = object({
  edges: array(ontologyEdge),
  links: array(ontologyLink),
  types: array(ontologyType),
});
export type OntologyResponse = Infer<typeof ontologyResponse>;

export const searchReindexResponse = object({
  document_count: number,
  file_count: number,
  folder_count: number,
  index_path: string,
  indexed_at: string,
  item_count: number,
  ok: boolean,
});
export type SearchReindexResponse = Infer<typeof searchReindexResponse>;

export const resolveResponse = object({
  folder: string,
  id: string,
  is_folder_index: boolean,
  type_folder: string,
});
export type ResolveResponse = Infer<typeof resolveResponse>;

export const searchFacetCount = object({
  count: number,
  facet: string,
});
export type SearchFacetCount = Infer<typeof searchFacetCount>;

export const searchMode = literals('keyword');
export type SearchMode = Infer<typeof searchMode>;

export const searchResultKind = literals('folder', 'document', 'file');
export type SearchResultKind = Infer<typeof searchResultKind>;

export const searchResult = object({
  extension: optional(string),
  facets: array(string),
  id: optional(string),
  kind: searchResultKind,
  path: string,
  score: number,
  snippet: optional(string),
  status: optional(string),
  title: optional(string),
  type: optional(string),
});
export type SearchResult = Infer<typeof searchResult>;

export const searchResponse = object({
  facets: array(searchFacetCount),
  mode: searchMode,
  query: string,
  results: array(searchResult),
});
export type SearchResponse = Infer<typeof searchResponse>;

export const searchStatus = object({
  document_count: number,
  exists: boolean,
  file_count: number,
  folder_count: number,
  index_path: string,
  item_count: number,
  last_indexed_at: optional(string),
});
export type SearchStatus = Infer<typeof searchStatus>;

export const schemaConstraints = object({
  allowed_actors: array(string),
  allowed_edge_predicates: array(string),
  allowed_status: array(string),
  allowed_types: array(string),
  notes: array(string),
});
export type SchemaConstraints = Infer<typeof schemaConstraints>;

export const nodeSchema = object({
  fields: mapOf(fieldSchema),
  required: array(string),
});
export type NodeSchema = Infer<typeof nodeSchema>;

export const tomlSchemaResponse = object({
  constraints: schemaConstraints,
  kind: string,
  node_schema: optional(nodeSchema),
  schema: json,
  toml_template: string,
});
export type TomlSchemaResponse = Infer<typeof tomlSchemaResponse>;

export const severity = literals('error', 'warning', 'info');
export type Severity = Infer<typeof severity>;

export const diagnostic = object({
  code: string,
  message: string,
  path: optional(string),
  severity: severity,
});
export type Diagnostic = Infer<typeof diagnostic>;

export const validateResponse = object({
  diagnostics: array(diagnostic),
  ok: boolean,
});
export type ValidateResponse = Infer<typeof validateResponse>;

export const vaultLimits = object({
  max_folder_depth: optional(number),
});
export type VaultLimits = Infer<typeof vaultLimits>;

export const scanConfig = object({
  ignore: array(string),
  use_default_ignores: boolean,
});
export type ScanConfig = Infer<typeof scanConfig>;

export const vaultIndex = object({
  created_at: optional(string),
  limits: vaultLimits,
  name: string,
  scan: scanConfig,
  schema_version: string,
  type_folders: mapOf(string),
  updated_at: optional(string),
});
export type VaultIndex = Infer<typeof vaultIndex>;
