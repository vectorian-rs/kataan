import {
  Archive,
  Boxes,
  Circle,
  FileText,
  FolderKanban,
  Lightbulb,
  User,
  createElement,
  type IconNode,
} from 'lucide';
import { marked } from 'marked';

import {
  getDocument,
  getFolder,
  getFolders,
  getVault,
  rebuildIndexes,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type FolderChild,
  type FolderDocument,
  type FolderSummary,
  type ValidateResponse,
} from './api';

const vaultSummary = requireElement<HTMLElement>('vault-summary');
const foldersEl = requireElement<HTMLElement>('folders');
const documentsEl = requireElement<HTMLElement>('documents');
const diagnosticsEl = requireElement<HTMLElement>('diagnostics');
const folderTitle = requireElement<HTMLElement>('folder-title');
const breadcrumb = requireElement<HTMLElement>('breadcrumb');
const documentTitle = requireElement<HTMLElement>('document-title');
const documentBody = requireElement<HTMLElement>('document-body');
const metadataPanel = requireElement<HTMLElement>('metadata-panel');
const validateButton = requireElement<HTMLButtonElement>('validate-button');
const rebuildButton = requireElement<HTMLButtonElement>('rebuild-button');

let selectedFolder: string | null = null;
let selectedDocument: string | null = null;
const loadedFolderIds = new Set<string>();

validateButton.addEventListener('click', async () => {
  await runAction(async () => {
    renderDiagnostics(await validateVault());
  });
});

rebuildButton.addEventListener('click', async () => {
  await runAction(async () => {
    await rebuildIndexes();
    await loadFolders();
    renderDiagnostics(await validateVault());
  });
});

await runAction(async () => {
  await loadVault();
  await loadFolders();
});

async function loadVault() {
  const vault = await getVault();
  vaultSummary.textContent = `${vault.name} · schema ${vault.schema_version}`;
}

async function loadFolders() {
  const response = await getFolders();
  foldersEl.replaceChildren(...response.folders.map(renderFolderButton));

  const firstNonEmptyFolder = response.folders.find((folder) => folder.document_count > 0);
  if (response.folders.length > 0) {
    await selectFolder(firstNonEmptyFolder?.folder ?? response.folders[0].folder);
  }
}

function renderFolderButton(folder: FolderSummary) {
  const button = clickableRow('nav-row');
  button.dataset.folder = folder.folder;

  const label = document.createElement('span');
  label.className = 'folder-name';

  const icon = document.createElement('span');
  icon.className = `folder-icon ${folder.type}`;
  icon.append(createElement(folderIcon(folder.type), { width: 18, height: 18, 'stroke-width': 2 }));

  const name = document.createElement('span');
  name.textContent = folder.name ?? folder.folder;

  label.append(icon, name);

  const badge = document.createElement('span');
  badge.className = 'badge';
  badge.textContent = String(folder.document_count);

  button.append(label, badge);
  button.addEventListener('click', () => runAction(() => selectFolder(folder.folder)));
  return button;
}

async function selectFolder(folder: string) {
  selectedFolder = folder;
  updateActiveRows();

  const response = await getFolder(folder);
  loadedFolderIds.add(folder);
  folderTitle.textContent = folderTitleFromResponse(response.id, response.metadata);
  renderChildFolders(folder, response.folders);

  if (response.documents.length === 0) {
    documentsEl.className = 'list muted section';
    documentsEl.textContent = response.folders.length === 0 ? 'No documents.' : 'Select a nested folder.';
    selectedDocument = null;
    updateActiveRows();
    return;
  }

  documentsEl.className = 'list';
  documentsEl.replaceChildren(...response.documents.map(renderDocumentButton));
  await selectDocument(response.documents[0].id);
}

function renderChildFolders(parentId: string, folders: FolderChild[]) {
  const parentRow = foldersEl.querySelector<HTMLElement>(`[data-folder="${cssEscape(parentId)}"]`);
  if (!parentRow) return;

  let insertAfter = parentRow;
  for (const folder of folders) {
    let row = foldersEl.querySelector<HTMLElement>(`[data-folder="${cssEscape(folder.id)}"]`);
    if (!row) {
      row = renderChildFolderButton(folder, depthFor(folder.id));
      insertAfter.after(row);
    }
    insertAfter = row;
  }
}

function renderChildFolderButton(folder: FolderChild, depth: number) {
  const button = clickableRow('nav-row nested');
  button.dataset.folder = folder.id;
  button.style.setProperty('--depth', String(depth));

  const label = document.createElement('span');
  label.className = 'folder-name';

  const icon = document.createElement('span');
  icon.className = 'folder-icon';
  icon.append(createElement(FolderKanban, { width: 18, height: 18, 'stroke-width': 2 }));

  const name = document.createElement('span');
  name.textContent = folder.name;

  label.append(icon, name);
  button.append(label);
  button.addEventListener('click', () => runAction(() => selectFolder(folder.id)));
  return button;
}

function renderDocumentButton(vaultDocument: FolderDocument) {
  const button = clickableRow('document-row');
  button.dataset.document = vaultDocument.id;

  const title = document.createElement('strong');
  title.textContent = titleFromSlug(vaultDocument.slug);

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = vaultDocument.id;

  button.append(title, meta);
  button.addEventListener('click', () => runAction(() => selectDocument(vaultDocument.id)));
  return button;
}

async function selectDocument(id: string) {
  selectedDocument = id;
  updateActiveRows();

  const vaultDocument = await getDocument(id);
  breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
  documentTitle.textContent = titleFromId(vaultDocument.id);
  renderDocumentBody(vaultDocument);
  renderMetadata(vaultDocument);
}

function renderDocumentBody(vaultDocument: DocumentResponse) {
  documentBody.className = 'reader-body';
  documentBody.innerHTML = '';

  documentBody.innerHTML = marked.parse(vaultDocument.markdown, { async: false });
}

function renderMetadata(vaultDocument: DocumentResponse) {
  metadataPanel.className = 'metadata-grid';
  metadataPanel.replaceChildren(
    property('ID', vaultDocument.id),
    ...Object.entries(vaultDocument.metadata).map(([key, value]) => property(formatLabel(key), value)),
  );
}

function property(label: string, value: unknown) {
  const wrapper = document.createElement('div');
  wrapper.className = 'property';

  const labelEl = document.createElement('div');
  labelEl.className = 'property-label';
  labelEl.textContent = label;

  const valueEl = renderPropertyValue(value);

  wrapper.append(labelEl, valueEl);
  return wrapper;
}

async function runAction(action: () => Promise<void>) {
  try {
    await action();
  } catch (error) {
    renderError(error);
  }
}

function renderDiagnostics(report: ValidateResponse) {
  if (report.diagnostics.length === 0) {
    diagnosticsEl.className = 'list muted';
    diagnosticsEl.textContent = report.ok ? 'Vault is valid.' : 'No diagnostics.';
    return;
  }

  diagnosticsEl.className = 'list';
  diagnosticsEl.replaceChildren(...report.diagnostics.map(renderDiagnostic));
}

function renderDiagnostic(diagnostic: Diagnostic) {
  const item = document.createElement('div');
  item.className = `diagnostic ${diagnostic.severity}`;
  item.textContent = `${diagnostic.severity.toUpperCase()} [${diagnostic.code}] ${diagnostic.path ?? ''} ${diagnostic.message}`;
  return item;
}

function renderError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);

  diagnosticsEl.className = 'list';
  const item = document.createElement('div');
  item.className = 'diagnostic error';
  item.textContent = message;
  diagnosticsEl.replaceChildren(item);

  documentTitle.textContent = 'Unable to load document';
  documentBody.className = 'reader-body';
  documentBody.textContent = message;
}

function clickableRow(className: string) {
  const row = document.createElement('div');
  row.className = className;
  row.role = 'button';
  row.tabIndex = 0;
  row.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      row.click();
    }
  });
  return row;
}

function updateActiveRows() {
  document.querySelectorAll<HTMLElement>('[data-folder]').forEach((row) => {
    row.classList.toggle('active', row.dataset.folder === selectedFolder);
  });

  document.querySelectorAll<HTMLElement>('[data-document]').forEach((row) => {
    row.classList.toggle('active', row.dataset.document === selectedDocument);
  });
}

function folderIcon(type: string): IconNode {
  const icons: Record<string, IconNode> = {
    note: FileText,
    person: User,
    project: FolderKanban,
    raw: Archive,
    topic: Lightbulb,
    'type-definition': Boxes,
  };
  return icons[type] ?? Circle;
}

function folderTitleFromResponse(id: string, metadata?: Record<string, unknown>) {
  const name = metadata?.name;
  return typeof name === 'string' && name.length > 0 ? name : titleFromId(id);
}

function depthFor(id: string) {
  return Math.max(0, id.split('/').length - 1);
}

function cssEscape(value: string) {
  return CSS.escape(value);
}

function titleFromId(id: string) {
  return titleFromSlug(id.split('/').at(-1) ?? id);
}

function titleFromSlug(slug: string) {
  return slug
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function formatLabel(label: string) {
  return label.replaceAll('_', ' ');
}

function renderPropertyValue(value: unknown) {
  const valueEl = document.createElement('div');
  valueEl.className = 'property-value';

  if (Array.isArray(value) && value.length > 0) {
    valueEl.classList.add('pill-list');
    valueEl.replaceChildren(...value.map(renderPill));
    return valueEl;
  }

  valueEl.textContent = formatValue(value);
  return valueEl;
}

function renderPill(value: unknown) {
  const pill = document.createElement('span');
  pill.className = 'pill';
  pill.textContent = String(value);
  return pill;
}

function formatValue(value: unknown): string {
  if (Array.isArray(value)) {
    return value.length === 0 ? '—' : value.join(', ');
  }

  if (value === null || value === undefined || value === '') {
    return '—';
  }

  if (typeof value === 'object') {
    return JSON.stringify(value, null, 2);
  }

  return String(value);
}

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing element #${id}`);
  }
  return element as T;
}
