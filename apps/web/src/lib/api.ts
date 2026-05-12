const API_BASE = import.meta.env.PUBLIC_KATAAN_API_BASE ?? '';

export type VaultIndex = {
  schema_version: string;
  name: string;
  created_at?: string;
  updated_at?: string;
  type_folders: Record<string, string>;
};

export type FolderSummary = {
  type: string;
  folder: string;
  name?: string;
  icon?: string;
  document_count: number;
};

export type FolderChild = {
  id: string;
  name: string;
  has_index: boolean;
};

export type FolderDocument = {
  id: string;
  slug: string;
  markdown: string;
  toml: string;
};

export type FolderFile = {
  name: string;
  path: string;
  extension?: string;
};

export type FolderResponse = {
  folder: string;
  index: {
    name: string;
    description?: string;
    default_type?: string;
    folder_checksum?: string;
  };
  documents: FolderDocument[];
};

export type CanonicalFolderResponse = {
  id: string;
  metadata?: Record<string, unknown>;
  markdown?: string;
  folders: FolderChild[];
  documents: FolderDocument[];
  files: FolderFile[];
};

export type DocumentResponse = {
  id: string;
  type_folder: string;
  route_token: string;
  metadata: Record<string, unknown>;
  markdown: string;
  html: string;
};

export type FileResponse = {
  path: string;
  name: string;
  extension?: string;
  kind: 'json' | 'text' | 'image' | 'unsupported';
  content: string;
};

export type HighlightResponse = {
  path: string;
  name: string;
  extension?: string;
  language: string;
  html: string;
};

export type ResolveResponse = {
  id: string;
  folder: string;
  type_folder: string;
  route_token: string;
  is_folder_index: boolean;
};

export type Diagnostic = {
  severity: 'error' | 'warning' | 'info';
  code: string;
  message: string;
  path?: string;
};

export type ValidateResponse = {
  ok: boolean;
  diagnostics: Diagnostic[];
};

export type TomlSchemaResponse = {
  kind: string;
  schema: Record<string, unknown>;
  constraints: {
    allowed_status: string[];
    allowed_actors: string[];
    allowed_types: string[];
    allowed_edge_predicates: string[];
    notes: string[];
  };
  toml_template: string;
};

export async function getVault() {
  return getJson<VaultIndex>('/api/vault');
}

export async function getFolders() {
  return getJson<{ folders: FolderSummary[] }>('/api/folders');
}

export async function getFolder(id: string) {
  return getJson<CanonicalFolderResponse>(`/api/folder?id=${encodeURIComponent(id)}`);
}

export async function getDocument(id: string) {
  return getJson<DocumentResponse>(`/api/document?id=${encodeURIComponent(id)}`);
}

export async function getFile(path: string) {
  return getJson<FileResponse>(`/api/file?path=${encodeURIComponent(path)}`);
}

export async function getHighlightedFile(path: string, theme?: string) {
  const params = new URLSearchParams({ path });
  if (theme) params.set('theme', theme);
  return getJson<HighlightResponse>(`/api/file/highlight?${params.toString()}`);
}

export function getRawFileUrl(path: string) {
  return `${API_BASE}/api/file/raw?path=${encodeURIComponent(path)}`;
}

export async function resolveRoute(type: string, token: string) {
  return getJson<ResolveResponse>(`/api/resolve?type=${encodeURIComponent(type)}&token=${encodeURIComponent(token)}`);
}

export async function getSchema(kind: string) {
  return getJson<TomlSchemaResponse>(`/api/schema/${encodeURIComponent(kind)}`);
}

export async function validateVault() {
  return postJson<ValidateResponse>('/api/validate');
}

export async function rebuildIndexes() {
  return postJson<{ ok: boolean }>('/api/rebuild-indexes');
}

async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`);
  return parseJson<T>(response);
}

async function postJson<T>(path: string): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, { method: 'POST' });
  return parseJson<T>(response);
}

async function parseJson<T>(response: Response): Promise<T> {
  if (response.ok) {
    return response.json() as Promise<T>;
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
