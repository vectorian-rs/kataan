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
  document_count: number;
};

export type FolderDocument = {
  id: string;
  slug: string;
  markdown: string;
  toml: string;
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

export type DocumentResponse = {
  id: string;
  metadata: Record<string, unknown>;
  markdown: string;
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

export async function getVault() {
  return getJson<VaultIndex>('/api/vault');
}

export async function getFolders() {
  return getJson<{ folders: FolderSummary[] }>('/api/folders');
}

export async function getFolder(folder: string) {
  return getJson<FolderResponse>(`/api/folders/${encodeURIComponent(folder)}`);
}

export async function getDocument(id: string) {
  return getJson<DocumentResponse>(`/api/documents/${id}`);
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
  if (!response.ok) {
    throw new Error(`API request failed: ${response.status}`);
  }
  return response.json() as Promise<T>;
}
