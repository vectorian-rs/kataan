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

export async function getFolder(id: string) {
  return getJson<CanonicalFolderResponse>(`/api/folder?id=${encodeURIComponent(id)}`);
}

export async function getDocument(id: string) {
  return getJson<DocumentResponse>(`/api/document?id=${encodeURIComponent(id)}`);
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
