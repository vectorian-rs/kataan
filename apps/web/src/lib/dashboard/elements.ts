//! Handles to the elements the shell renders into, resolved once at load.

export function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing element #${id}`);
  }
  return element as T;
}

export const appShell = requireElement<HTMLElement>('app-shell');
export const sidebarResizeHandle = requireElement<HTMLElement>('sidebar-resize-handle');
export const listResizeHandle = requireElement<HTMLElement>('list-resize-handle');
export const propertiesResizeHandle = requireElement<HTMLElement>('properties-resize-handle');
export const propertiesToggle = requireElement<HTMLButtonElement>('properties-toggle');
export const vaultSummary = requireElement<HTMLElement>('vault-summary');
export const foldersEl = requireElement<HTMLElement>('folders');
export const documentsEl = requireElement<HTMLElement>('documents');
export const diagnosticsEl = requireElement<HTMLElement>('diagnostics');
export const folderTitle = requireElement<HTMLElement>('folder-title');
export const breadcrumb = requireElement<HTMLElement>('breadcrumb');
export const documentTitle = requireElement<HTMLElement>('document-title');
export const documentBody = requireElement<HTMLElement>('document-body');
export const metadataPanel = requireElement<HTMLElement>('metadata-panel');
export const schemaPanel = requireElement<HTMLElement>('schema-panel');
export const validateButton = requireElement<HTMLButtonElement>('validate-button');
export const rebuildButton = requireElement<HTMLButtonElement>('rebuild-button');
export const searchForm = requireElement<HTMLFormElement>('search-form');
export const searchInput = requireElement<HTMLInputElement>('search-input');
export const searchStatusEl = requireElement<HTMLElement>('search-status');
